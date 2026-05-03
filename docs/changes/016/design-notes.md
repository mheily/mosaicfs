# Change 016: Design Notes

> Implementation specifics for the umbrella architecture in
> `architecture.md`. Part 016 produces no code — these notes finalize
> the schema, intent-log row format, sync wire format, snapshot format,
> compaction policy, and config-leader mechanics so parts 017–020 can
> be implemented from a stable spec.

---

## 1. Conventions

### Storage and connection
- Two SQLite files live in `${data_dir}/`:
  - **`mosaicfs-local.db`** — this node's private data, never replicated:
    `local_node` (identity + private key) and `sync_state` (per-origin
    high-water marks).
  - **`mosaicfs-cluster.db`** — everything that participates in cluster
    state: `intent_log`, all derived tables, `peer`, `cluster_meta`.
  Both run WAL mode, `synchronous=NORMAL`, `foreign_keys=ON`.
- Connections open the local DB and `ATTACH` the cluster DB as
  `cluster`:
  ```rust
  let conn = Connection::open(&local_db_path)?;
  conn.execute_batch(&format!(
      "ATTACH DATABASE '{}' AS cluster", cluster_db_path,
  ))?;
  ```
  Queries reference `local_node` / `sync_state` directly; cluster
  tables are `cluster.file`, `cluster.intent_log`, etc.
- **Cross-DB atomicity is intentionally not used.** WAL mode does not
  guarantee atomic commit across attached databases, so we structure
  writes to keep each transaction inside a single file (cluster
  mutations + intent_log together in cluster.db; sync_state updates
  separately in local.db). Idempotent replay (intent_log PK
  `(origin_node_id, sequence_no)`) covers the crash-between-files
  case — see §3 Replay.
- Splitting the files means **full-sync recovery only swaps
  `mosaicfs-cluster.db`**; local identity and per-origin high-water
  state are preserved. No identity surgery on the imported snapshot.
- The VFS file/block cache (`mosaicfs-vfs/src/cache.rs`,
  `cache/index.db`) is a third, independent SQLite file unaffected by
  this design.
- Timestamps are stored as TEXT in RFC 3339 UTC (`'2026-05-02T21:40:01Z'`)
  to match the existing `chrono::DateTime<Utc>` serialization. Indexes
  on timestamp columns work correctly in lexicographic order.
- JSON columns store `serde_json::Value` payloads where the original
  document already used free-form JSON (e.g., `subsystems`, `backend_config`,
  plugin `config`/`settings`). New code paths should not introduce more
  JSON columns without justification.
- `mosaicfs-common::db` exposes typed accessors. Handler code never
  calls `rusqlite` directly — it calls a typed function like
  `db::file::insert(&tx, &record)`.

### Identifiers
- **Node identity is a small integer** (`node_id INTEGER`), allocated
  sequentially by the cluster leader.
  - The founding node is `node_id = 1` and is automatically the
    founding leader. It writes its own `local_node` row at first run;
    no allocator needed.
  - Subsequent joiners receive their `node_id` from the leader during
    pairing. The leader computes `MAX(node_id) + 1` over the union of
    `local_node` and `peer` (a stateless query — see §2 sequence
    tables) inside the same transaction that writes the new `peer`
    row, then ships the assigned id back to the joiner.
  - IDs are never reused. Even if a node is decommissioned and removed
    from the peer table, peers still hold intent-log rows tagged with
    its `origin_node_id`; recycling would alias the old node's writes
    to the new node. AUTOINCREMENT enforces this.
  - `node_id = 0` is reserved as a sentinel for "unassigned" (used
    in-memory before a fresh node completes its first pairing).
- **No `cluster_id` in v1.** The "two pre-existing clusters cannot
  pair" safety check (resolved decision 6) is enforced by the rule:
  *neither side may have a non-empty peer set when joining*. If
  multi-cluster awareness ever becomes necessary, `cluster_id` can be
  added as a single column on `local_node` later.
- **Composite keys are used for replicated entities.** Anything minted
  by a specific node uses `(origin_node_id INTEGER, origin_id INTEGER)`
  as its primary key. Examples: `file (origin_node_id, origin_file_id)`,
  `replica (origin_node_id, origin_file_id, target_name)`, etc.
  - The origin node mints the second component locally (see "ID
    allocation" below), then ships the row through the intent log.
  - Peers receive the composite verbatim and insert it as-is. Peers
    never modify origin-assigned IDs.
  - The composite PK guarantees global uniqueness because the
    `origin_node_id` qualifier disambiguates IDs minted by different
    nodes.
- **URL/wire format** for composite IDs is bare `<node_id>-<origin_file_id>`
  (e.g., `2-17`). Routes stay shaped as `/api/files/{file_id}`; the
  route name disambiguates the type, so no `file:` scheme prefix in
  URLs. Parsers split on `-`. The Rust newtype's `Display` impl may
  prepend a typed prefix (`file:2-17`) for log lines where context is
  ambiguous, but that prefix is never part of the URL contract.
- **User-chosen names stay TEXT** (storage backend names, plugin
  names, label rule names, replication rule names, friendly_name).
  Cardinality is small and the user picks them.
- **`origin_node_id` column is required** on every replicated table.
  For node-sharded tables it equals the writing node. For shared-config
  tables it equals the leader at write time.

### ID allocation (stateless `MAX(...) + 1`)
We don't keep separate per-entity-type sequence tables. Local IDs are
allocated by querying the cluster table directly inside the writer
transaction:
```sql
-- inside BEGIN IMMEDIATE on the cluster.db connection
SELECT COALESCE(MAX(origin_file_id), 0) + 1
  FROM cluster.file WHERE origin_node_id = :me;
-- use the result as the new origin_file_id
INSERT INTO cluster.file (...) VALUES (...);
INSERT INTO cluster.intent_log (...) VALUES (...);
COMMIT;
```
Same pattern for `intent_log.sequence_no` (`MAX FROM cluster.intent_log
WHERE origin_node_id = :me`), `replica`, `annotation`, `notification`,
and the leader-only `node_id` allocation in §2.

**Why stateless beats AUTOINCREMENT in this design:**
- The two-file split would otherwise make `local_seq_*` a separate
  surface that could drift from cluster.db state. After a full-sync
  recovery, AUTOINCREMENT counters in local.db would point below the
  IDs already present in the restored cluster.db, causing collisions.
- Querying cluster data is the source of truth — nothing to seed, no
  handoff on leader change, no recovery surgery.
- Reuse is prevented because the relevant tables either soft-delete
  (`file.status = 'deleted'`) or never delete (`intent_log` uses
  compaction, but sequence numbers post-compaction are still bounded
  by current `MAX`). The composite PK on the entity table additionally
  prevents collisions if a stale value somehow gets minted.

The composite PK on the entity table handles the *collision* concern
(peer rows and local rows coexist without conflict). The MAX query is
the *generator* for the second component. The two mechanisms are
orthogonal.

### Rust ergonomics
Composite keys are wrapped in newtypes so handler code stays clean:
```rust
pub struct NodeId(pub i32);
pub struct FileId   { pub node: NodeId, pub origin_id: i64 }
pub struct ReplicaId{ pub file: FileId, pub target: String }
// etc.

impl FileId {
    pub fn parse(s: &str) -> Result<Self, IdError> { /* "2-17" */ }
    pub fn as_url(&self) -> String { format!("{}-{}", self.node.0, self.origin_id) }
}

impl std::fmt::Display for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "file:{}-{}", self.node.0, self.origin_id)
    }
}
```
Typed accessors take and return these newtypes; raw column tuples
never appear in handler signatures.

---

## 2. Cluster metadata tables

Per the file split in §1: `local_node` and `sync_state` live in
`mosaicfs-local.db`; `peer` and `cluster_meta` live in
`mosaicfs-cluster.db` (shared config, written by the leader, replicated
via the intent log like any other shared-config table). This split is
what lets full-sync recovery swap only the cluster file without
touching local identity.

### `local_node`
Singleton row describing this node.
```sql
CREATE TABLE local_node (
    rowid INTEGER PRIMARY KEY CHECK (rowid = 1),
    node_id INTEGER NOT NULL UNIQUE,     -- 1 for the founding node, otherwise leader-assigned
    private_key BLOB NOT NULL,           -- Ed25519, 32 bytes
    public_key BLOB NOT NULL,            -- Ed25519, 32 bytes
    created_at TEXT NOT NULL
);
```
- `node_id` follows the allocation rules in §1 (founder = 1, joiners
  get their id from the leader at pair time, never reused, `0`
  reserved as the unassigned sentinel).
- The keypair material lives here on Linux. On macOS the private key
  is stored in the Keychain (keyed by `node_id`) and this column holds
  a placeholder; loader code reads from Keychain when the placeholder
  is detected.

### `peer`
Every paired remote peer (excluding this node itself, which is in
`local_node`).
```sql
CREATE TABLE peer (
    node_id INTEGER PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,     -- = leader at write time
    public_key BLOB NOT NULL,
    last_known_endpoint TEXT,            -- e.g. "192.168.1.10:8443"
    paired_at TEXT NOT NULL
);
```
Stores the TOFU pubkey set the rustls verifier consults.

Writes go through the leader (peer registration is shared config —
see §7 pairing flow). `last_known_endpoint` is the one column updated
locally without going through the leader: each node refreshes it on
successful contact, and the user can edit it from the UI to point at
a moved peer. This is the same local-write exception used for
`credential.last_seen`. All other peer-table fields require leader
writes.

### `sync_state`
Per-origin high-water mark this node has caught up to.
```sql
CREATE TABLE sync_state (
    origin_node_id INTEGER PRIMARY KEY,  -- includes our own node_id
    high_water_seq INTEGER NOT NULL,     -- last sequence_no applied
    last_sync_at TEXT
);
```
The receiver bumps `high_water_seq` in the same transaction that applies
a batch of intent-log rows from `origin_node_id`. On first run this row
is created with `high_water_seq = 0`.

### `cluster_meta`
Singleton row holding cluster-wide settings that everyone needs at boot.
```sql
CREATE TABLE cluster_meta (
    rowid INTEGER PRIMARY KEY CHECK (rowid = 1),
    leader_node_id INTEGER NOT NULL,
    leader_set_at TEXT NOT NULL,
    schema_version INTEGER NOT NULL
);
```
Updated via a special intent-log entry (`set_leader`) that all nodes
replay. Leader change requires user confirmation in the UI; not a
silent failover.

### No persistent sequence tables
All ID allocation is stateless `MAX(...) + 1` queries against
cluster.db (see §1 "ID allocation"). This includes node IDs:
```sql
-- leader-only, runs inside the pairing transaction
SELECT COALESCE(MAX(node_id), 0) + 1
  FROM (SELECT node_id FROM local_node
        UNION SELECT node_id FROM cluster.peer);
```
Any node that becomes leader immediately knows the next id to hand out
— the peer table is shared-config and stays consistent. No allocator
handoff on leader change.

---

## 3. Intent log

Single append-only table. Every node owns its own rows
(`origin_node_id = local_node.node_id`); rows from peers are inserted
only via sync replay.

```sql
CREATE TABLE intent_log (
    origin_node_id INTEGER NOT NULL,
    sequence_no INTEGER NOT NULL,
    op_type TEXT NOT NULL,               -- 'put' | 'delete' | 'set_leader'
    entity_type TEXT NOT NULL,           -- e.g. 'file', 'replica', 'node_runtime'
    entity_key TEXT NOT NULL,            -- canonical string form of the entity's PK (see §4)
    payload TEXT NOT NULL,               -- JSON; full record for 'put', empty object for 'delete'
    timestamp TEXT NOT NULL,
    PRIMARY KEY (origin_node_id, sequence_no)
);

CREATE INDEX idx_intent_log_entity ON intent_log(entity_type, entity_key);
CREATE INDEX idx_intent_log_timestamp ON intent_log(timestamp);
```

### Sequence number generation
Per-origin monotonic. The next local `sequence_no` is computed inside
the writer transaction as
`SELECT COALESCE(MAX(sequence_no), 0) + 1 FROM cluster.intent_log
WHERE origin_node_id = :me`. No persistent counter — see §1
"ID allocation" for why.

### `entity_key`
A canonical string form of the entity's primary key, used solely for
the `idx_intent_log_entity` index (e.g., to look up "all log entries
that touched file 2-17"). Format depends on entity type:
- `file`: `"<origin_node_id>-<origin_file_id>"` (e.g., `"2-17"`)
- `replica`: `"<origin_node_id>-<origin_file_id>-<target_name>"`
- `agent_status`, `node_runtime`, `node_config`: `"<node_id>"`
- `virtual_directory`: the virtual_path
- `storage_backend`, `plugin`, `label_rule`, `replication_rule`,
  `credential`: their natural string PK

The replay logic decodes `entity_key` per `entity_type`, so the
canonical form is purely an index key — `payload` carries the full
typed record.

### Atomicity invariant
Every mutation to a derived table is committed in the same transaction
as its `intent_log` insert. The writer task (single-thread, pool size 1
for writes per `architecture.md` §1) wraps both in `BEGIN IMMEDIATE …
COMMIT`. Property test: replay the intent log from scratch into an
empty database and assert the derived tables match the original.

### Replay
Two transactions per batch — one in cluster.db, one in local.db. We
intentionally don't span the file boundary (see §1 "Storage and
connection" on cross-DB atomicity).

For a batch of N rows from `origin_node_id`:
1. `BEGIN IMMEDIATE` on the cluster.db connection.
2. For each row in `sequence_no` order:
   - If `op_type = 'put'`: upsert into the derived table using the PK
     decoded from `payload` (which carries the full record).
   - If `op_type = 'delete'`: hard `DELETE` from the derived table
     using the PK decoded from `entity_key`.
   - If `op_type = 'set_leader'`: apply to `cluster.cluster_meta`
     (requires the user to have confirmed; enforced at submit time by
     the leader's own check, not by replay).
   - Insert the row into `cluster.intent_log` (idempotent — discarded
     if `(origin_node_id, sequence_no)` already exists).
3. `COMMIT` cluster.db.
4. Separately: `UPDATE sync_state SET high_water_seq = ?, last_sync_at
   = ? WHERE origin_node_id = ?` in local.db.

**Crash between steps 3 and 4** leaves cluster.db committed but
sync_state stale. On restart the next sync request runs against the
old (lower) high-water; the peer returns rows we already have; the
receiver's loop in step 2 is idempotent (intent_log PK rejects the
duplicates, derived-table upserts are no-ops because the data is
identical). Sync_state catches up on the next successful batch. No
correctness loss; at most one redundant batch fetch on the rare
crash-between-files window.

**Crash mid-batch (during step 2)** rolls back the cluster.db
transaction; the next sync resumes from the (still accurate) sync_state
high-water.

### Compaction
Per resolved decision 3: each node compacts its own intent log on a
timer.
- Default thresholds: rows older than **30 days** OR table size over
  **100 MB**, whichever fires first.
- Both knobs live in the `[sync]` section of the TOML config.
- Compaction is purely local: `DELETE FROM intent_log WHERE
  origin_node_id = local_node.node_id AND timestamp < cutoff`.
- A peer that falls behind and discovers its high-water is below the
  oldest remaining `sequence_no` for an origin must fall back to full
  sync (see §6). Detection: GET `/sync/highwater` returns each origin's
  `min_sequence_no` available, and the receiver compares against its
  own `sync_state`.

---

## 4. Derived tables (per document type)

Each table maps a single `documents.rs` struct (or split, in the case
of `NodeDocument`). All sharing tables include `origin_node_id`. All
shared-config tables include it too — for those, it's always the
current leader's id.

Tables are listed below in no particular order. Field names stay
identical to the Rust struct fields (snake_case) so handler code doesn't
have to translate.

### Node-sharded tables

#### `file`
PK is composite `(origin_node_id, origin_file_id)` per §1.
`origin_file_id` is minted via the stateless `MAX(...) + 1` query
(see §1) for locally-discovered files and copied verbatim from peer
rows during replay.
```sql
CREATE TABLE file (
    origin_node_id INTEGER NOT NULL,
    origin_file_id INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    name TEXT NOT NULL,
    source_export_path TEXT NOT NULL,
    source_export_parent TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime TEXT NOT NULL,
    mime_type TEXT,
    status TEXT NOT NULL,                -- 'active' | 'deleted'
    deleted_at TEXT,
    migrated_from_node_id INTEGER,
    migrated_from_origin_file_id INTEGER,
    migrated_from_export_path TEXT,
    migrated_from_at TEXT,
    PRIMARY KEY (origin_node_id, origin_file_id)
);
CREATE INDEX idx_file_export_parent ON file(source_export_parent);
CREATE INDEX idx_file_status ON file(status);
```
Note: today's `source.node_id` field is dropped — it was always equal
to `origin_node_id` since file records are owned by the indexing node.
The composite PK already encodes that ownership. `name` does not need
its own index until FTS5 is added (deferred). `idx_file_origin` is
unnecessary because `origin_node_id` is the leading column of the PK
(SQLite uses the PK index for `WHERE origin_node_id = ?` queries).

#### `agent_status`
One row per node. Latest-only; older snapshots are not kept.
```sql
CREATE TABLE agent_status (
    node_id INTEGER PRIMARY KEY,         -- == origin_node_id
    updated_at TEXT NOT NULL,
    overall TEXT NOT NULL,
    subsystems TEXT NOT NULL,            -- JSON
    recent_errors TEXT NOT NULL          -- JSON array
);
```
Single-node-owned table — `node_id` is sufficient as PK; no separate
`origin_node_id` column needed (it is `node_id` by definition).

#### `utilization_snapshot`
Append-only history. PK is `(node_id, captured_at)`.
```sql
CREATE TABLE utilization_snapshot (
    node_id INTEGER NOT NULL,            -- == origin_node_id
    captured_at TEXT NOT NULL,
    filesystems TEXT,                    -- JSON array, nullable
    cloud TEXT,                          -- JSON object, nullable
    PRIMARY KEY (node_id, captured_at)
);
CREATE INDEX idx_util_node_time ON utilization_snapshot(node_id, captured_at DESC);
```
A separate retention policy (independent of intent-log compaction)
prunes snapshots older than N days; this is local maintenance, not a
sync concern.

#### `access`
**Schema change from CouchDB:** today's `access::<file_uuid>` ID
implicitly assumes one record per file globally. Under additive sync
this would race between nodes. New PK adds the observing node.
```sql
CREATE TABLE access (
    file_origin_node_id INTEGER NOT NULL,
    file_origin_file_id INTEGER NOT NULL,
    observer_node_id INTEGER NOT NULL,   -- == origin_node_id of this row
    last_access TEXT NOT NULL,
    access_count INTEGER NOT NULL,
    PRIMARY KEY (file_origin_node_id, file_origin_file_id, observer_node_id)
);
CREATE INDEX idx_access_file ON access(file_origin_node_id, file_origin_file_id);
```
The existing `AccessCache` aggregates by file — sum or max as appropriate.
This is a behavior-preserving change at the cache layer (the
aggregation already had to handle multi-node observations because the
bulk-write in `flush_access_records` could overwrite other nodes'
work).

#### `notification`
**Schema change:** today's `notification::<node_id>::<condition_key>`
ID makes node_id load-bearing. Make it explicit in the schema.
```sql
CREATE TABLE notification (
    node_id INTEGER NOT NULL,            -- == origin_node_id
    condition_key TEXT NOT NULL,
    component TEXT NOT NULL,
    severity TEXT NOT NULL,
    status TEXT NOT NULL,
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    actions TEXT,                        -- JSON, nullable
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL,
    acknowledged_at TEXT,
    resolved_at TEXT,
    PRIMARY KEY (node_id, condition_key)
);
```

#### `node_runtime` (split from `NodeDocument`)
Node-owned runtime state. One row per node, written only by that node.
```sql
CREATE TABLE node_runtime (
    node_id INTEGER PRIMARY KEY,         -- == origin_node_id
    status TEXT NOT NULL,                -- 'online' | 'offline' | 'degraded'
    last_heartbeat TEXT NOT NULL,
    platform TEXT NOT NULL,
    capabilities TEXT NOT NULL,          -- JSON array
    vfs_capable INTEGER NOT NULL,        -- 0/1
    vfs_backend TEXT,
    storage TEXT,                        -- JSON array of StorageEntry, nullable
    network_mounts TEXT                  -- JSON array of NetworkMount, nullable
);
```
Rationale for which fields are runtime: every field listed above is
either observed by the node (heartbeat, status, storage measurements)
or determined by the node's binary build/environment (platform,
capabilities, vfs_capable, vfs_backend). Network mounts are
runtime-discovered too.

### Shared-config tables (writes go through the leader)

#### `node_config` (split from `NodeDocument`)
User-owned settings about each node.
```sql
CREATE TABLE node_config (
    node_id INTEGER PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,     -- = leader at write time
    friendly_name TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```
Today's `NodeDocument` has only `friendly_name` as a clearly user-set
field. If more user-settable per-node config emerges (e.g., per-node
labels, per-node limits) it gets columns here.

#### `virtual_directory`
```sql
CREATE TABLE virtual_directory (
    virtual_path TEXT PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    name TEXT NOT NULL,
    parent_path TEXT,
    system INTEGER,                      -- 0/1, nullable
    enforce_steps_on_children INTEGER NOT NULL,
    mounts TEXT NOT NULL,                -- JSON array of MountEntry
    created_at TEXT NOT NULL
);
CREATE INDEX idx_vdir_parent ON virtual_directory(parent_path);
```
`mounts` stays as JSON; the structure is rich (nested Steps with
op-specific params) and is read whole into memory by the VFS on every
resolve. Splitting into normalized tables would buy nothing.

#### `filesystem`
```sql
CREATE TABLE filesystem (
    filesystem_id TEXT PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,
    friendly_name TEXT NOT NULL,
    owning_node_id INTEGER NOT NULL,
    export_root TEXT NOT NULL,
    availability TEXT NOT NULL,          -- JSON array of NodeAvailability
    created_at TEXT NOT NULL
);
```
`availability` stays JSON — it's a cluster-wide aggregate (all known
nodes' mount info for this filesystem) updated by the leader from
heartbeats. Note: this means heartbeat receipt at the leader triggers
a leader write, generating an intent-log entry. Acceptable rate; if
volume becomes a problem, batch and rate-limit.

#### `label_rule`
```sql
CREATE TABLE label_rule (
    rule_id TEXT PRIMARY KEY,            -- 'label_rule::<uuid>'
    origin_node_id INTEGER NOT NULL,
    applies_to_node_id INTEGER,          -- which node the rule applies to (NULL = all)
    path_prefix TEXT NOT NULL,
    labels TEXT NOT NULL,                -- JSON array
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_label_rule_node ON label_rule(applies_to_node_id, enabled);
```
Today's `'*'` sentinel for "applies to all nodes" becomes `NULL` —
INTEGER columns can't hold a `'*'` string.

#### `label_assignment`
PK is the composite file PK.
```sql
CREATE TABLE label_assignment (
    file_origin_node_id INTEGER NOT NULL,
    file_origin_file_id INTEGER NOT NULL,
    origin_node_id INTEGER NOT NULL,     -- = leader at write time
    labels TEXT NOT NULL,                -- JSON array
    updated_at TEXT NOT NULL,
    updated_by TEXT NOT NULL,            -- credential access_key_id
    PRIMARY KEY (file_origin_node_id, file_origin_file_id)
);
```

#### `credential`
```sql
CREATE TABLE credential (
    access_key_id TEXT PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,
    secret_key_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    last_seen TEXT,
    permissions_scope TEXT NOT NULL
);
```
`last_seen` is updated locally on every authenticated request; it is
**not** replicated (would generate write storms). It's the one
shared-config column treated as local-only — handlers update it
without writing to `intent_log`.

---

## 5. Sync API wire format

All sync endpoints live under `/sync/*`. Authentication is the new
peer-cert verifier (part 019); until then, the routes are mounted
behind a feature flag and reject all non-loopback traffic.

Per resolved decision 4: JSON over HTTPS. Per resolved decision 7:
disjoint from the existing `/api/*` routes; HMAC verifier does not
apply here.

### `GET /sync/highwater`
Returns this node's view of every origin in its `intent_log`.

Response:
```json
{
  "node_id": 1734511023,
  "origins": [
    { "origin_node_id": 482910377, "min_seq": 1024, "max_seq": 5871 },
    { "origin_node_id": 1734511023, "min_seq": 1, "max_seq": 12044 }
  ]
}
```
- `min_seq` is the oldest sequence_no still present (post-compaction).
  Receiver compares against its `sync_state.high_water_seq`: if lower
  than `min_seq - 1`, fall back to full sync.
- `max_seq` is what the receiver should pull up to.

### `GET /sync/log?origin=<node_id>&since=<seq>&limit=<N>`
Returns intent-log rows from one origin in `sequence_no` order.

Response:
```json
{
  "origin_node_id": 482910377,
  "rows": [
    {
      "sequence_no": 1025,
      "op_type": "put",
      "entity_type": "file",
      "entity_key": "482910377-17",
      "payload": { "...full file record..." },
      "timestamp": "2026-05-01T12:00:00Z"
    }
  ],
  "next_since": 1525
}
```
- `limit` is server-capped (default 1000, max 10000).
- `next_since` is `last_returned_seq + 1`. Client repeats until it
  matches `max_seq` from `/sync/highwater`.
- Empty `rows` with `next_since == since` means caller is caught up.

### `GET /sync/snapshot`
Per resolved decision 5: streaming SQLite backup file plus a JSON
manifest header. Returned as `multipart/mixed` for simplicity:

```
--mfs-snapshot
Content-Type: application/json

{
  "manifest_version": 1,
  "source_node_id": 482910377,
  "anchor_high_water": {
    "482910377": 5871,
    "1734511023": 0
  },
  "schema_version": 1,
  "snapshot_size_bytes": 41875234,
  "captured_at": "2026-05-02T22:00:00Z"
}
--mfs-snapshot
Content-Type: application/octet-stream

<raw SQLite backup file bytes>
--mfs-snapshot--
```

Receiver behavior:
1. Read manifest, verify the receiver has not already paired with
   different peers — only a node with an empty `cluster.peer` table
   may accept a snapshot (enforces resolved decision 6: two
   pre-existing clusters cannot merge).
2. Verify `schema_version` matches.
3. Stream the body to a temp file inside `data_dir`.
4. Open the temp file as a SQLite database, validate it has the
   expected tables.
5. **Atomic swap of the cluster file only:** stop the writer task,
   close the cluster connection, rename `mosaicfs-cluster.db` to
   `mosaicfs-cluster.db.bak.<ts>`, rename the temp file to
   `mosaicfs-cluster.db`, re-`ATTACH` it on the live local connection,
   restart the writer. `mosaicfs-local.db` is untouched, so this
   node's identity, keypair, and per-origin sync state are preserved
   automatically.
6. Replace `sync_state` rows in local.db from `anchor_high_water`.
7. Resume incremental sync from the anchor.

Source behavior (snapshot generation):
1. `BEGIN IMMEDIATE` — gets a write lock briefly.
2. `SELECT origin_node_id, MAX(sequence_no) FROM intent_log GROUP BY
   origin_node_id` — capture anchor.
3. `VACUUM INTO 'snapshot-<uuid>.db'` — produces a consistent file
   while holding the lock; releases when complete.
4. Stream the file out, then unlink.
5. If the source compacts during a slow stream, that's fine — the
   already-materialized file is unaffected. The receiver just resumes
   from the captured anchor regardless.

### `POST /sync/pair/initiate` and `POST /sync/pair/confirm`
Pairing flow (detailed in part 019). Mentioned here for completeness;
both endpoints are unauthenticated but rate-limited and only accept
short-lived pairing tokens.

---

## 6. Sync client behavior

A single tokio task per node, started at boot.

```
loop {
    for peer in peers {
        if peer unreachable: continue
        try_incremental(peer)  // see below
        if incremental returned NeedFullSync(origin): try_full_sync(peer)
    }
    sleep(backoff)
}
```

**Incremental:**
1. `GET /sync/highwater` from peer.
2. Verify the peer's `node_id` is in our local `peer` table (paired).
3. For each origin in the response:
   - If our `sync_state.high_water_seq < peer.min_seq - 1` →
     `NeedFullSync(origin)`.
   - Else page `/sync/log?origin=...&since=...` until caught up.
4. Each batch is applied per the replay procedure in §3.

**Backoff:** start at 5 s, double on failure to 5 min cap; reset on
success. On idle (caught up across all peers and all origins), poll
every 30 s.

**Full sync** is only used when (a) joining a new cluster or (b)
detecting compaction past our high-water for any origin. It downloads
the entire snapshot from the peer and atomically swaps cluster.db.
Because cluster.db includes our own intent log, full sync replaces
this node's own historical writes with the peer's record of them.
Therefore: **full sync should only be initiated by a node that has no
local writes worth keeping.** The first-run join flow qualifies; the
recovery case requires a UI confirmation.

### Loss window during full-sync recovery

Worth being explicit about because the framing can mislead: file index
data is **node-owned**, not leader-owned. The leader has no special
role in file replication. Full-sync from a peer is "fetch this peer's
cached view of our past writes," not "fetch authoritative state from
the leader."

The loss window is precisely:
- `mosaicfs-cluster.db` is destroyed (corruption, accidental delete,
  or an explicit user choice to reset), AND
- our most recent crawler/replication writes hadn't yet been pulled by
  any peer when the loss happened, AND
- the user invokes full-sync recovery rather than restoring from a
  filesystem-level backup.

The unshipped window is bounded by the sync poll interval (~30s in
steady state). What's lost is *file index entries* — the actual files
remain on disk, and a re-crawl after recovery picks them up. We do not
lose user data, only at most one poll interval's worth of indexing
work.

Mitigations available later if the window matters:
1. **Push notification.** Sender pokes peers ("I have new entries,
   pull now") after each commit. Drops the window to round-trip time.
2. **Pre-recovery push.** Before initiating full-sync, push our
   outstanding `intent_log` rows to the chosen peer first; the
   subsequent snapshot then includes them. Useful when cluster.db is
   only partially lost (e.g., a slightly stale backup restore).

Both are intentionally deferred. The current model is: ship the simple
bidirectional pull protocol; observe whether the loss scenario bites
in practice; add either mitigation if it does.

**What we are not adding:** a separate "pending writes" table in
local.db that the crawler writes to first and then flushes to the
cluster file table. That pattern would double the write path, force
every UI/FUSE/search query to UNION across two tables, and add a
flush worker — all to protect against a narrow disaster recovery
window where the consequence is a re-crawl. Not worth the permanent
complexity.

---

## 7. Config-leader forwarding

Two patterns in handler code, depending on whether the node is leader.

### Server-side: every shared-config write handler
```rust
async fn create_label_rule(state, req) -> Result<Response> {
    if state.is_leader() {
        let mut tx = state.db.begin_immediate().await?;
        db::label_rule::insert(&mut tx, &record)?;
        intent_log::append(&mut tx, "put", "label_rule",
                           &record.rule_id, &record)?;
        tx.commit().await?;
        Ok(Response::ok(record))
    } else {
        match state.peers.forward_to_leader(req).await {
            Ok(resp) => Ok(resp),
            Err(LeaderUnreachable) => Err(ApiError::LeaderOffline {
                leader_node_id: state.cluster_meta().leader_node_id,
            }),
        }
    }
}
```
The error renderer looks up the leader's `friendly_name` from
`node_config` for the user-facing message; if the leader's row is not
yet present (early-boot edge case), falls back to `format!("node {}",
leader_node_id)`.

`ApiError::LeaderOffline` renders as a 503 in JSON responses and as a
flash message in HTMX responses: *"Configuration changes require
\<leader-friendly-name\> to be online. Reads still work."*

### Forwarder
A small helper in `mosaicfs-server::peers`. Holds a rustls client
configured with the peer cert verifier. Forwards the request body
verbatim to the leader's same path; returns the leader's response
verbatim. The forwarder runs for shared-config write routes — the
route table marks each route as `LocalOnly | LeaderOnly | LeaderForwarded`.

### Pairing (also leader-only)
Joining a new node into the cluster requires the leader, because only
the leader can mint a new `node_id` and write the `peer` row (peer
registry is shared config). Same forwarder pattern as config edits:
the user can point the joining node at any existing peer, and that
peer forwards the pair request to the leader. If the leader is
unreachable, the joiner sees the same `ApiError::LeaderOffline` flash
message used for config edits, with copy adjusted: *"Adding a node
requires <leader-name> to be online."*

Pair flow detail:
1. Joiner generates its Ed25519 keypair locally; `node_id` is `0`
   (sentinel) at this stage.
2. Joiner POSTs `/sync/pair/initiate` to any cluster peer, including
   its public key and a short fingerprint.
3. The receiving peer (leader or forwarder) presents the joiner's
   fingerprint to the operator on that side for out-of-band
   confirmation; symmetrically, the joiner displays the leader's
   fingerprint for confirmation on its side.
4. After both confirmations, the leader allocates `node_id` per the
   `MAX(...) + 1` query in §2, writes the new `peer` row, appends a
   `put`-on-`peer` intent-log entry, and returns the assigned
   `node_id` to the joiner.
5. Joiner persists `node_id` in `local_node` and proceeds to full sync.

### Leader change
A new admin route `POST /api/cluster/promote-self-to-leader` that:
1. Requires explicit confirmation (HTMX dialog with a "type the node
   name to confirm" pattern).
2. Writes a `set_leader` intent log entry locally.
3. Updates local `cluster_meta.leader_node_id`.
4. The next sync cycle propagates the change.

If the previous leader is online and disagrees (e.g., it also got
promoted), the cluster ends up in conflict. Detection: `set_leader`
entries from two different origins in the same epoch. Resolution: the
later `leader_set_at` wins; admin notification is raised. This is the
one place a timestamp matters; document the time-sync expectation
(NTP) in the operator docs.

---

## 8. Cache invalidation replacement

Today's `_changes` watcher (`mosaicfs-server/src/start.rs:264-322`)
fans out to `LabelCache`, `AccessCache`, and `readdir_cache`.

Replacement: a small in-process pub/sub channel. The writer task emits
an event after each commit:
```rust
enum DbEvent {
    Inserted { entity_type: &'static str, entity_key: String },
    Updated  { entity_type: &'static str, entity_key: String },
    Deleted  { entity_type: &'static str, entity_key: String },
}
```
`entity_key` follows the same canonical-string format used in the
intent log (§3). Subscribers (the three caches) filter by `entity_type`
and react.

Sync replay also emits these events after a batch commits, so
peer-driven changes invalidate caches the same way. No polling, no
2-second latency.

The `_conflicts` notification path is removed entirely — there are no
conflicts under additive sync.

---

## 9. UI changes (deferred to Part 020 implementation)

Listed here so design-notes captures the surface area:

- **Replace** `desktop/ui/setup.html` with a create-or-join screen.
  Equivalent web-UI bootstrap added at `/ui/bootstrap`.
- **Update** `templates/status_panel.html` to show "Database: SQLite
  (local)" instead of CouchDB status.
- **Update** `templates/settings_backup.html` copy to describe SQLite
  backup file copies instead of CouchDB JSON dumps.
- **Add** `templates/cluster.html` for peer list + pairing UX.
- **Add** flash-message rendering for the new `ApiError::LeaderOffline`
  case across all shared-config edit forms.
- **Remove** the `/db/{*path}` proxy route and any UI links that point
  at it.

---

## 10. Open implementation choices for Part 017

These do not require architectural sign-off; flagging them so the
implementer knows where judgment calls live.

- **`rusqlite` vs `sqlx`.** Existing code uses `rusqlite`. Stay with it
  for Part 017. `sqlx` async would be nicer but the writer task is
  single-threaded anyway, and `tokio::task::spawn_blocking` around
  `rusqlite` calls is fine.
- **Migrations framework.** Hand-rolled `schema_version` check at
  startup is sufficient given there's only the v1 schema; reach for a
  proper migrations crate when v2 lands.
- **In-memory DB for tests.** `Connection::open_in_memory()`. The
  existing test fixtures that spin up CouchDB containers
  (`tests/docker-compose.integration.yml`) become unnecessary for unit
  tests but the integration tests still need to be rewritten to
  exercise two real pairs of `mosaicfs-local.db` + `mosaicfs-cluster.db`
  files.
- **Connection pool sizing.** Single writer connection (matches the
  writer-task design). Read pool: start at 4, tune later.
