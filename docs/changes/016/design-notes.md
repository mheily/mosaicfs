# Change 016: Design Notes

> Implementation specifics for the umbrella architecture in
> `architecture.md`. Part 016 produces no code — these notes finalize
> the database split, schemas, intent-log row format, sync wire format,
> snapshot formats, compaction policy, and config-leader mechanics so
> parts 018–021 can be implemented from a stable spec. (Part 017 is the
> database testsuite that validates the strategy below before any
> application code is written.)

---

## 1. Conventions

### Three databases, three ownership models

Every node holds three SQLite files in `${data_dir}/`. Each has a
distinct owner, lifetime, and replication mechanism. The strict
separation is what makes the rest of the design simple.

| File | Owner | Lifetime | Replication |
|------|-------|----------|-------------|
| `mosaicfs-local.db`   | this node only | preserved across all recoveries | never replicated |
| `mosaicfs-cluster.db` | gossip cache   | regenerable from peers | per-row via intent-log replay |
| `mosaicfs-config.db`  | leader-only writes | regenerable from any peer with higher version | whole-file snapshot, version-stamped |

**`mosaicfs-local.db`** holds this node's authoritative sharded data
(file index entries we crawled, our own status/runtime/notifications),
our private identity (keypair, node_id), our authoritative intent log
(record of *our* writes), and our per-origin sync high-water marks.
Lost only via user-controlled disk failure; never overwritten by sync.

**`mosaicfs-cluster.db`** holds peer-replicated sharded data (file
index entries crawled by other nodes) and a relay cache of peer
intent-log entries. Entirely regenerable: if it's lost, full-sync from
any peer and resume.

**`mosaicfs-config.db`** holds shared configuration (peer registry,
label rules, virtual directories, filesystems, credentials, per-node
config). Single-writer (the leader). Replicated as a whole-file
snapshot stamped with `(epoch, version)`. Any peer with a higher
version can serve it to any peer with a lower version.

The VFS file/block cache (`mosaicfs-vfs/src/cache.rs`,
`cache/index.db`) is a fourth, independent SQLite file that pre-dates
this change and is unaffected.

### Connection layout

Every connection opens an empty in-memory database as `main` and
ATTACHes all three real files under named schemas:

```rust
let conn = Connection::open_in_memory()?;
conn.execute_batch(&format!(
    "ATTACH DATABASE '{local_path}'   AS local;
     ATTACH DATABASE '{cluster_path}' AS cluster;
     ATTACH DATABASE '{config_path}'  AS config;",
))?;
```

This is deliberate. The `main` schema is kept empty so that **every
table reference must be qualified** (`local.file`, `cluster.file`,
`config.label_rule`). An accidental unqualified `SELECT * FROM file`
in handler code raises `no such table: main.file` at parse time
rather than silently targeting the wrong schema. The convention is
enforced by the database, not by code review.

All three real files run WAL mode, `synchronous=NORMAL`,
`foreign_keys=ON`. Views (§5) are stored in `local` and reference
`local.<table>` and `cluster.<table>` by qualified name; they work
regardless of whatever the `main` schema is.

On non-leader nodes, `mosaicfs-config.db` is ATTACHed with the
`SQLITE_OPEN_READONLY` flag (via `ATTACH ... AS config` after opening
the file separately, or by ATTACHing a URI with `?mode=ro`). On the
leader, `READWRITE`. Leadership change DETACHes and re-ATTACHes
config with the new mode. This turns "only the leader writes config"
from a code-convention invariant into a database-enforced one.

### One transaction per file (atomicity invariant)

**No transaction ever spans more than one of the three database
files.** WAL mode does not guarantee atomic commit across attached
databases, so we structure all writes to keep each transaction inside
one file. The combinations actually used:

| Event | File touched | Transaction shape |
|-------|--------------|-------------------|
| Crawler indexes a local file | local.db | `local.file` insert + `local.intent_log` append |
| Receive peer intent-log row | cluster.db | `cluster.<entity>` upsert + `cluster.intent_log` append |
| Heartbeat / status update | local.db | `local.<entity>` upsert + `local.intent_log` append |
| Leader edits shared config | config.db | `config.<entity>` write + `config.config_meta.version` bump (+ `config.config_audit` row) |
| sync_state high-water bump | local.db | `local.sync_state` update only |
| config snapshot installed | local.db | `local.config_state` update only (after the file-system swap) |
| Pre-restore quiesce / atomic swap | (none) | file-system rename + ATTACH |

Crash-between-files scenarios are covered by idempotent replay on the
intent-log path (see §7) and version-monotonic gossip on the config
path (see §10). Cross-file atomicity is never assumed.

### Storage encoding

- **Timestamps:** TEXT in RFC 3339 UTC (`'2026-05-02T21:40:01Z'`),
  matching `chrono::DateTime<Utc>`'s default serialization.
  Lexicographic order matches chronological order; standard indexes
  work directly on these columns.
- **JSON columns:** kept where the source struct already uses
  free-form JSON (`subsystems`, `mounts`, `recent_errors`,
  `permissions_scope`). New code paths should not introduce more
  JSON columns without justification.
- **Booleans:** INTEGER 0/1.
- **Typed accessors only:** `mosaicfs-common::db` exposes typed query
  functions. Handler code never calls `rusqlite` directly — it calls
  `db::file::insert(&tx, &record)` or `db::file::list_by_parent(&conn,
  parent)`. The accessor layer is the single place that knows which
  physical table or view to target for a given operation.

### Identifiers

Identifier rules are unchanged from the previous draft:

- **`node_id INTEGER`**, allocated sequentially by the leader, never
  reused, `0` reserved as the unassigned sentinel.
- **Composite keys** `(origin_node_id, origin_id)` for replicated
  entities minted by a specific node (`file`, etc.). The composite is
  globally unique because `origin_node_id` qualifies IDs minted by
  different nodes independently.
- **Wire/URL format** for composite IDs: bare `<node>-<id>` (e.g.,
  `2-17`). Routes stay shaped as `/api/files/{file_id}`.
- **User-chosen names** stay TEXT (storage backend names, label rule
  names, friendly_name).
- **Stateless `MAX(...) + 1`** for ID allocation, run inside the
  writer transaction. No persistent counter tables. Querying current
  data is the source of truth, which means no AUTOINCREMENT counter
  drift after a recovery.

### Rust ergonomics

```rust
pub struct NodeId(pub i32);
pub struct FileId   { pub node: NodeId, pub origin_id: i64 }
pub struct ReplicaId{ pub file: FileId, pub target: String }
// etc.

impl FileId {
    pub fn parse(s: &str) -> Result<Self, IdError> { /* "2-17" */ }
    pub fn as_url(&self) -> String { format!("{}-{}", self.node.0, self.origin_id) }
}
```

Typed accessors take and return these newtypes; raw column tuples
never appear in handler signatures.

---

## 2. `mosaicfs-local.db` schema

All `CREATE` statements below run against a connection that has
already ATTACHed the local file as `local`. Every statement
qualifies its target with `local.<name>`.

### `local.local_node`
Singleton row describing this node.
```sql
CREATE TABLE local.local_node (
    rowid INTEGER PRIMARY KEY CHECK (rowid = 1),
    node_id INTEGER NOT NULL UNIQUE,     -- 1 for the founding node, otherwise leader-assigned
    private_key BLOB NOT NULL,           -- Ed25519, 32 bytes (placeholder on macOS — see below)
    public_key BLOB NOT NULL,
    created_at TEXT NOT NULL
);
```
On Linux the private key lives here. On macOS it lives in the Keychain
(keyed by `node_id`) and this column holds a placeholder byte string;
the loader detects the placeholder and fetches from Keychain.

### `local.sync_state`
Per-origin high-water mark this node has caught up to, for the intent
log only.
```sql
CREATE TABLE local.sync_state (
    origin_node_id INTEGER PRIMARY KEY,
    high_water_seq INTEGER NOT NULL,     -- last sequence_no applied from this origin
    last_sync_at TEXT
);
```
The receiver bumps `high_water_seq` in a separate transaction after
the cluster.db replay batch commits (see §7). On first run a row is
created with `high_water_seq = 0` for each known peer.

### `local.config_state`
Tracks the `mosaicfs-config.db` snapshot this node currently has
installed. Singleton row.
```sql
CREATE TABLE local.config_state (
    rowid INTEGER PRIMARY KEY CHECK (rowid = 1),
    leader_epoch INTEGER NOT NULL,
    version INTEGER NOT NULL,
    installed_at TEXT NOT NULL
);
```
A separate table from `local.sync_state` because the comparison key
is `(leader_epoch, version)` rather than a single sequence number,
and the install path (atomic file swap) is unrelated to intent-log
batching. Updated in a one-row transaction after the config snapshot
swap completes (§7).

### `local.intent_log`
This node's authoritative log of its own writes. No `origin_node_id`
column — every row is owned by us implicitly.
```sql
CREATE TABLE local.intent_log (
    sequence_no INTEGER PRIMARY KEY,
    op_type TEXT NOT NULL,               -- 'put' | 'delete'
    entity_type TEXT NOT NULL,           -- 'file' | 'agent_status' | 'node_runtime' | ...
    entity_key TEXT NOT NULL,            -- canonical PK string (see §6)
    payload TEXT NOT NULL,               -- JSON full record on 'put', '{}' on 'delete'
    timestamp TEXT NOT NULL
);
CREATE INDEX local.idx_local_intent_log_entity     ON intent_log(entity_type, entity_key);
CREATE INDEX local.idx_local_intent_log_timestamp  ON intent_log(timestamp);
```
This log only ever contains rows authored by this node. It is the
record we serve to peers via `/sync/log?origin=<me>`.

### Sharded data tables (this node's authoritative rows)

All carry the invariant `origin_node_id = local_node.node_id`,
enforced in the typed accessor layer rather than via a SQL `CHECK`
constraint (since `my_node_id` isn't a schema-time constant). The
accessor refuses to insert a row whose `origin_node_id` differs from
`local.local_node.node_id`.

#### `local.file`
```sql
CREATE TABLE local.file (
    origin_node_id INTEGER NOT NULL,     -- = local.local_node.node_id
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
CREATE INDEX local.idx_local_file_export_parent ON file(source_export_parent);
CREATE INDEX local.idx_local_file_status        ON file(status);
```

#### `local.agent_status`, `local.node_runtime`
Singleton-per-node tables. Only ever hold this node's own row.
```sql
CREATE TABLE local.agent_status (
    node_id INTEGER PRIMARY KEY,
    updated_at TEXT NOT NULL,
    overall TEXT NOT NULL,
    subsystems TEXT NOT NULL,            -- JSON
    recent_errors TEXT NOT NULL          -- JSON array
);

CREATE TABLE local.node_runtime (
    node_id INTEGER PRIMARY KEY,
    status TEXT NOT NULL,                -- 'online' | 'offline' | 'degraded'
    last_heartbeat TEXT NOT NULL,
    platform TEXT NOT NULL,
    capabilities TEXT NOT NULL,          -- JSON array
    vfs_capable INTEGER NOT NULL,
    vfs_backend TEXT,
    storage TEXT,                        -- JSON array of StorageEntry
    network_mounts TEXT,                 -- JSON array of NetworkMount
    filesystem_mounts TEXT               -- JSON array — see §11 schema cleanup
);
```

#### `local.utilization_snapshot`
Append-only history this node has captured.
```sql
CREATE TABLE local.utilization_snapshot (
    node_id INTEGER NOT NULL,
    captured_at TEXT NOT NULL,
    filesystems TEXT,                    -- JSON, nullable
    cloud TEXT,                          -- JSON, nullable
    PRIMARY KEY (node_id, captured_at)
);
CREATE INDEX local.idx_local_util_node_time
    ON utilization_snapshot(node_id, captured_at DESC);
```

#### `local.notification`
Notifications raised by this node. PK is `(node_id, condition_key)`
matching the legacy CouchDB `notification::<node_id>::<condition_key>` shape.
```sql
CREATE TABLE local.notification (
    node_id INTEGER NOT NULL,
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

#### `local.access`
Per-file access observations recorded by this node.
```sql
CREATE TABLE local.access (
    file_origin_node_id INTEGER NOT NULL,
    file_origin_file_id INTEGER NOT NULL,
    observer_node_id INTEGER NOT NULL,   -- = local.local_node.node_id
    last_access TEXT NOT NULL,
    access_count INTEGER NOT NULL,
    PRIMARY KEY (file_origin_node_id, file_origin_file_id, observer_node_id)
);
CREATE INDEX local.idx_local_access_file
    ON access(file_origin_node_id, file_origin_file_id);
```

#### `local.credential_observed`
Per-credential `last_seen` observations, recorded per-node. Replaces
the `credential.last_seen` column from the previous draft (see §11).
```sql
CREATE TABLE local.credential_observed (
    access_key_id TEXT NOT NULL,
    observer_node_id INTEGER NOT NULL,   -- = local.local_node.node_id
    last_seen TEXT NOT NULL,
    request_count INTEGER NOT NULL,
    PRIMARY KEY (access_key_id, observer_node_id)
);
```

### `local.schema_meta`
Each of the three databases carries its own version row to support
independent schema evolution.
```sql
CREATE TABLE local.schema_meta (
    rowid INTEGER PRIMARY KEY CHECK (rowid = 1),
    schema_version INTEGER NOT NULL
);
```

---

## 3. `mosaicfs-cluster.db` schema

This database is a gossip cache. Every table mirrors the schema of its
`local.*` counterpart but holds rows authored by **other** nodes (i.e.,
`origin_node_id != my_node_id`). The accessor layer enforces this
disjointness on insert; the union with the local table is exposed via
views (see §5).

### `cluster.intent_log`
Relay cache of peer intent-log entries. Carries `origin_node_id`
because it stores rows from many origins.
```sql
CREATE TABLE cluster.intent_log (
    origin_node_id INTEGER NOT NULL,
    sequence_no INTEGER NOT NULL,
    op_type TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    PRIMARY KEY (origin_node_id, sequence_no)
);
CREATE INDEX cluster.idx_cluster_intent_log_entity
    ON intent_log(entity_type, entity_key);
CREATE INDEX cluster.idx_cluster_intent_log_timestamp
    ON intent_log(timestamp);
```

### Sharded data tables (peer rows)

Same column shapes as the matching `local.*` tables. Indexes mirror
the local indexes — every predicate that has an index in local must
have the equivalent index in cluster, otherwise view queries do an
indexed scan on one branch and a full scan on the other.

```sql
-- cluster.file: identical schema to local.file, stores peer-authored rows
CREATE TABLE cluster.file (
    origin_node_id INTEGER NOT NULL,
    origin_file_id INTEGER NOT NULL,
    /* ... same columns as local.file ... */
    PRIMARY KEY (origin_node_id, origin_file_id)
);
CREATE INDEX cluster.idx_cluster_file_export_parent ON file(source_export_parent);
CREATE INDEX cluster.idx_cluster_file_status        ON file(status);

-- cluster.agent_status: one row per peer node
CREATE TABLE cluster.agent_status (
    node_id INTEGER PRIMARY KEY,
    /* ... same columns as local.agent_status ... */
);

-- cluster.node_runtime, cluster.utilization_snapshot,
-- cluster.notification, cluster.access, cluster.credential_observed
-- all follow the same pattern: CREATE TABLE cluster.<name>,
-- same columns as local.<name>, indexes mirrored as
-- CREATE INDEX cluster.idx_cluster_<...>.
```

### `cluster.schema_meta`
Same shape as `local.schema_meta`, qualified to the cluster schema.

---

## 4. `mosaicfs-config.db` schema

Single-writer (leader). All tables in this database are rewritten by
whole-file snapshot replication; no intent log is involved.

### `config.config_meta`
Carries the version stamp used for snapshot ordering.
```sql
CREATE TABLE config.config_meta (
    rowid INTEGER PRIMARY KEY CHECK (rowid = 1),
    version INTEGER NOT NULL,            -- bumps on every leader-write transaction
    leader_node_id INTEGER NOT NULL,
    leader_epoch INTEGER NOT NULL,       -- bumps on each leader change
    last_modified_at TEXT NOT NULL
);
```
Ordering rule: `(leader_epoch, version)` is a Lamport pair. A snapshot
with `(e1, v1)` is newer than one with `(e2, v2)` iff `e1 > e2`, or
`e1 == e2 && v1 > v2`. The single-writer-per-epoch invariant
guarantees this is total within an epoch; epoch bumps require user
confirmation (see §10) and resolve cross-epoch ordering.

### `config.peer`
Paired-peer registry. Excludes this node (which is in
`local.local_node`).
```sql
CREATE TABLE config.peer (
    node_id INTEGER PRIMARY KEY,
    public_key BLOB NOT NULL,
    last_known_endpoint TEXT,
    paired_at TEXT NOT NULL
);
```

Note: `last_known_endpoint` was previously called out as a "local
write exception" because nodes refresh it on successful contact. With
config.db, even the leader can't update it on every contact without
forcing a cluster-wide snapshot ship. Solution: the contact-time
endpoint goes into `local.node_runtime` / `cluster.node_runtime` as
part of the per-node runtime state (gossiped via intent log at
acceptable rate). The `config.peer` row carries only the user-edited
/ pairing-time endpoint — true config.

### `config.node_config`
User-set per-node settings.
```sql
CREATE TABLE config.node_config (
    node_id INTEGER PRIMARY KEY,
    friendly_name TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

### `config.virtual_directory`
```sql
CREATE TABLE config.virtual_directory (
    virtual_path TEXT PRIMARY KEY,
    inode INTEGER NOT NULL,
    name TEXT NOT NULL,
    parent_path TEXT,
    system INTEGER,                      -- 0/1, nullable
    enforce_steps_on_children INTEGER NOT NULL,
    mounts TEXT NOT NULL,                -- JSON array of MountEntry
    created_at TEXT NOT NULL
);
CREATE INDEX config.idx_vdir_parent ON virtual_directory(parent_path);
```

### `config.filesystem`
```sql
CREATE TABLE config.filesystem (
    filesystem_id TEXT PRIMARY KEY,
    friendly_name TEXT NOT NULL,
    owning_node_id INTEGER NOT NULL,
    export_root TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```
The `availability` field from the previous draft is removed —
heartbeat-driven aggregation belongs in sharded data, not config (see
§11).

### `config.label_rule`
```sql
CREATE TABLE config.label_rule (
    rule_id TEXT PRIMARY KEY,
    applies_to_node_id INTEGER,          -- NULL = all nodes
    path_prefix TEXT NOT NULL,
    labels TEXT NOT NULL,                -- JSON array
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX config.idx_label_rule_node ON label_rule(applies_to_node_id, enabled);
```

### `config.label_assignment`
PK is the composite file PK.
```sql
CREATE TABLE config.label_assignment (
    file_origin_node_id INTEGER NOT NULL,
    file_origin_file_id INTEGER NOT NULL,
    labels TEXT NOT NULL,                -- JSON array
    updated_at TEXT NOT NULL,
    updated_by TEXT NOT NULL,            -- credential access_key_id
    PRIMARY KEY (file_origin_node_id, file_origin_file_id)
);
```

### `config.credential`
```sql
CREATE TABLE config.credential (
    access_key_id TEXT PRIMARY KEY,
    secret_key_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    permissions_scope TEXT NOT NULL
);
```
The previous draft's `last_seen` column moves to
`local.credential_observed` / `cluster.credential_observed` (see §11).

### `config.config_audit`
Append-only audit trail of every leader write. Rides along in the
snapshot, so every peer carries a copy.
```sql
CREATE TABLE config.config_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    actor TEXT NOT NULL,                 -- credential access_key_id, or 'system'
    action TEXT NOT NULL,                -- 'create' | 'update' | 'delete'
    entity_type TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    before_json TEXT,                    -- nullable on create
    after_json TEXT                      -- nullable on delete
);
CREATE INDEX config.idx_config_audit_entity
    ON config_audit(entity_type, entity_key, timestamp DESC);
CREATE INDEX config.idx_config_audit_time
    ON config_audit(timestamp DESC);
```
Bounded by leader-side compaction (see §10): rows older than 90 days
are pruned at the same time `version` is bumped. Compaction rides in
the next snapshot like any other change.

### `config.schema_meta`
Same shape as `local.schema_meta`, qualified to the config schema.

---

## 5. Views

Views are stored in the `local` schema (so they survive any
cluster.db swap) and reference both `local.<table>` and
`cluster.<table>` by qualified name. Every connection ATTACHes
cluster.db and config.db before issuing any query — the views
require it. After a cluster.db swap (full-sync recovery, §9), the
connection DETACHes and re-ATTACHes the new file; the view
definitions are unchanged.

The `<view>_view` references must be qualified at use sites too —
e.g., `SELECT * FROM local.file_view`. Cross-schema view definitions
in SQLite live in the schema named in the `CREATE VIEW` statement;
storing them in `local` means a fresh-init flow only has to define
them once on the local file.

```sql
CREATE VIEW local.file_view AS
    SELECT * FROM local.file
    UNION ALL
    SELECT * FROM cluster.file;

CREATE VIEW local.agent_status_view AS
    SELECT * FROM local.agent_status
    UNION ALL
    SELECT * FROM cluster.agent_status;

CREATE VIEW local.node_runtime_view AS
    SELECT * FROM local.node_runtime
    UNION ALL
    SELECT * FROM cluster.node_runtime;

CREATE VIEW local.utilization_snapshot_view AS
    SELECT * FROM local.utilization_snapshot
    UNION ALL
    SELECT * FROM cluster.utilization_snapshot;

CREATE VIEW local.notification_view AS
    SELECT * FROM local.notification
    UNION ALL
    SELECT * FROM cluster.notification;

CREATE VIEW local.access_view AS
    SELECT * FROM local.access
    UNION ALL
    SELECT * FROM cluster.access;

CREATE VIEW local.credential_observed_view AS
    SELECT * FROM local.credential_observed
    UNION ALL
    SELECT * FROM cluster.credential_observed;
```

> Open question for Part 017: whether SQLite permits a view stored in
> one schema to reference tables in another attached schema across
> all the operations we need (planner pushdown into both branches,
> `EXPLAIN QUERY PLAN`, schema-evolution `ALTER TABLE` on a referenced
> table). The testsuite verifies this directly. If it doesn't, the
> fallback is to define the views as `TEMP VIEW` at connection setup
> time on every open. Functionally equivalent, slightly more boot
> work.

### Read paths

- "All files" (UI list, FUSE readdir, search): query `local.file_view`.
- "Files I crawled" (crawler reconciliation, my-stats panels): query
  `local.file` directly.
- "Files crawled by node X" where `X != me`: query `cluster.file`
  directly with `WHERE origin_node_id = X`.

The accessor layer chooses the right target. Handlers don't see the
split.

### Write paths

Views are not writable. Writes always target the physical table
chosen by the accessor:

- Local-origin write → `local.<table>`.
- Replay of peer-origin row → `cluster.<table>`.
- Leader write to shared config → `config.<table>`.

No INSTEAD OF triggers. The path is always explicit at the accessor
boundary.

### Index discipline

Every index that exists on a `local.*` table must exist on the
matching `cluster.*` table with the same definition. The Part 017
testsuite verifies that view queries use both indexes for predicate
pushdown; missing parity on either side compounds linearly with row
count.

---

## 6. Intent log (split)

### Roles

- **`local.intent_log`** — authoritative log of this node's own
  writes. Single source of truth for what this node has put on the
  wire. Survives any cluster.db loss.
- **`cluster.intent_log`** — relay cache of peer-authored entries.
  Fed by sync replay. Used to answer `/sync/log?origin=<peer>` when
  the origin is offline (gossip property).

### Sequence number generation
Per-origin monotonic. The next local `sequence_no` is computed inside
the writer transaction:
```sql
SELECT COALESCE(MAX(sequence_no), 0) + 1 FROM local.intent_log;
```
No persistent counter (see §1 "Stateless `MAX(...) + 1`"). Peer
sequence numbers in `cluster.intent_log` are copied verbatim from the
sender; the receiver never mints a peer's sequence numbers.

### `entity_key`
Canonical string form of the entity's PK, used by
`idx_<...>_intent_log_entity`. Format depends on `entity_type`:
- `file`: `"<origin_node_id>-<origin_file_id>"` (e.g., `"2-17"`)
- `agent_status`, `node_runtime`: `"<node_id>"`
- `utilization_snapshot`: `"<node_id>:<captured_at>"`
- `notification`: `"<node_id>:<condition_key>"`
- `access`: `"<file_origin_node_id>-<file_origin_file_id>:<observer_node_id>"`
- `credential_observed`: `"<access_key_id>:<observer_node_id>"`

The replay logic decodes `entity_key` per `entity_type`; `payload`
carries the full typed record.

### Replay (peer entries → my cluster.db)

For a batch of N rows pulled from peer P's `/sync/log?origin=X`:

1. `BEGIN IMMEDIATE` on cluster.db.
2. For each row in `sequence_no` order:
   - If `op_type = 'put'`: upsert into the matching `cluster.<entity>`
     table using PK from payload. Refuse the write if `origin_node_id
     == my_node_id` — that would be a peer trying to mint a row in
     our space, which shouldn't happen and indicates a bug or attack.
   - If `op_type = 'delete'`: hard `DELETE` from `cluster.<entity>`
     using PK decoded from `entity_key`.
   - Insert row into `cluster.intent_log` (idempotent — the PK
     `(origin_node_id, sequence_no)` rejects duplicates).
3. `COMMIT` cluster.db.
4. Separately, in local.db:
   `UPDATE local.sync_state SET high_water_seq = ?, last_sync_at = ?
    WHERE origin_node_id = ?`.

**Crash between steps 3 and 4** leaves cluster.db committed but
sync_state stale. Next sync re-pulls; cluster.intent_log PK rejects
duplicates; derived-table upserts are no-ops because data is
identical. No correctness loss; one redundant batch on the rare
crash-between-files window.

**Crash mid-batch** rolls back the cluster.db transaction; next sync
resumes from the (still accurate) sync_state high-water.

### Atomicity invariant property test

Replay an entire intent log (local + cluster sequence) into a
freshly-initialized pair of databases and assert the resulting
derived tables match the originals. Part 017 builds the harness; Part
018 wires it to the real schema.

### Compaction

Each node compacts its own logs on a timer:
- **`local.intent_log`**: rows older than **30 days** OR table size
  over **100 MB**, whichever fires first. These thresholds live in
  the `[sync]` section of TOML config.
- **`cluster.intent_log`**: more aggressive — purely a relay cache,
  so the policy can be "drop anything older than 7 days that all
  peers have already pulled past." Conservative default for v1: same
  thresholds as local; tune later.

A peer that falls behind and discovers its sync_state high-water for
some origin is below the oldest remaining `sequence_no` for that
origin (in any peer it asks) must fall back to full sync. Detection:
`/sync/highwater` returns each origin's `min_sequence_no` available;
the receiver compares against its own `sync_state`.

---

## 7. Sync API wire format

All sync endpoints live under `/sync/*`. Authentication is the new
peer-cert verifier (Part 020). Until then, the routes are mounted
behind a feature flag and reject all non-loopback traffic.

JSON over HTTPS. Disjoint from the existing `/api/*` routes; HMAC
verifier does not apply.

### `GET /sync/highwater`
Returns this node's view of every origin in either intent log it
holds (its own log + relayed peer logs).

```json
{
  "node_id": 2,
  "origins": [
    { "origin_node_id": 1, "min_seq": 1024, "max_seq": 5871 },
    { "origin_node_id": 2, "min_seq": 1,    "max_seq": 12044 }
  ],
  "config": { "leader_epoch": 3, "version": 47 }
}
```
- For `origin_node_id == self`, data comes from `local.intent_log`.
- For other origins, data comes from `cluster.intent_log`.
- `config.leader_epoch` and `config.version` advertise the snapshot
  this node currently has installed (see §10 for the gossip contract).
- `min_seq` is the oldest sequence_no still present (post-compaction).

### `GET /sync/log?origin=<node_id>&since=<seq>&limit=<N>`
Returns intent-log rows from one origin in `sequence_no` order.

If `origin == self`, served from `local.intent_log` (no
`origin_node_id` column needed; injected into response). Otherwise
from `cluster.intent_log` filtered by `origin`.

```json
{
  "origin_node_id": 1,
  "rows": [
    {
      "sequence_no": 1025,
      "op_type": "put",
      "entity_type": "file",
      "entity_key": "1-17",
      "payload": { "...full record..." },
      "timestamp": "2026-05-01T12:00:00Z"
    }
  ],
  "next_since": 1525
}
```
- `limit` is server-capped (default 1000, max 10000).
- `next_since` is `last_returned_seq + 1`. Client repeats until
  `next_since > max_seq` from `/sync/highwater`.

### `GET /sync/snapshot` (cluster.db full sync)
Streaming SQLite backup file of `mosaicfs-cluster.db` plus a JSON
manifest. `multipart/mixed`:
```
--mfs-snapshot
Content-Type: application/json
{
  "manifest_version": 1,
  "kind": "cluster",
  "source_node_id": 1,
  "anchor_high_water": { "1": 5871, "2": 12044 },
  "schema_version": 1,
  "snapshot_size_bytes": 41875234,
  "captured_at": "2026-05-02T22:00:00Z"
}
--mfs-snapshot
Content-Type: application/octet-stream
<raw SQLite backup file bytes>
--mfs-snapshot--
```
Receiver: only a node with an empty `config.peer` table may accept a
snapshot from a peer it has not paired with. Validate
`schema_version`; stream to a temp file inside `data_dir`; quiesce the
writer; close the cluster connection; rename
`mosaicfs-cluster.db` → `mosaicfs-cluster.db.bak.<ts>`; rename temp
file to `mosaicfs-cluster.db`; re-ATTACH; resume. Replace
`local.sync_state` rows from `anchor_high_water`. Local.db is
untouched, so identity,
keypair, and authoritative log are preserved.

Source: `BEGIN IMMEDIATE` on cluster.db; capture `(origin_node_id,
MAX(sequence_no))`; `VACUUM INTO 'snapshot-<uuid>.db'`; stream out;
unlink. If the source compacts during a slow stream, the
already-materialized file is unaffected.

### `GET /config/version`
Lightweight poll endpoint.
```json
{ "leader_node_id": 1, "leader_epoch": 3, "version": 47, "last_modified_at": "..." }
```

### `GET /config/snapshot` (config.db whole-file replication)
```
--mfs-config
Content-Type: application/json
{
  "manifest_version": 1,
  "kind": "config",
  "leader_node_id": 1,
  "leader_epoch": 3,
  "version": 47,
  "schema_version": 1,
  "snapshot_size_bytes": 184320,
  "captured_at": "2026-05-02T22:00:00Z"
}
--mfs-config
Content-Type: application/octet-stream
<raw SQLite backup file bytes of mosaicfs-config.db>
--mfs-config--
```
Receiver verifies `(leader_epoch, version)` is strictly newer than
the currently installed pair; quiesces config readers; closes the
config connection; renames `mosaicfs-config.db` →
`mosaicfs-config.db.bak.<ts>`; renames temp to
`mosaicfs-config.db`; reopens (READONLY on non-leaders); resumes.
Updates the singleton row in `local.config_state` with the new
`(leader_epoch, version, installed_at)` in a one-row transaction
inside local.db.

Snapshots are tiny (config.db is expected at KB to low-MB scale), so
shipping the whole file on every change is acceptable. If config.db
ever crosses ~10 MB the design admits a row-level diff endpoint as a
follow-up; we don't build it preemptively.

### `POST /sync/pair/initiate` and `POST /sync/pair/confirm`
Pairing flow (detailed in Part 020). Mentioned here for completeness;
both endpoints are unauthenticated but rate-limited and only accept
short-lived pairing tokens.

---

## 8. Sync client behavior

A single tokio task per node, started at boot.

```
loop {
    for peer in peers {
        if peer unreachable: continue
        try_incremental(peer)
        if incremental returned NeedFullSync(origin): try_cluster_full_sync(peer)
        try_config_refresh(peer)
    }
    sleep(backoff)
}
```

**Incremental intent-log sync:**
1. `GET /sync/highwater` from peer.
2. Verify peer's `node_id` is in `config.peer` (paired).
3. For each origin in the response:
   - If our `sync_state.high_water_seq < peer.min_seq - 1` →
     `NeedFullSync(origin)`.
   - Else page `/sync/log?origin=...&since=...` until caught up.
4. Each batch applied per the replay procedure in §6.

**Config refresh:**
1. From the same `/sync/highwater` response, read
   `peer.config.{leader_epoch, version}`.
2. If `(peer_epoch, peer_version) > (mine_epoch, mine_version)`:
   `GET /config/snapshot`, validate, atomic-swap (§7).

**Cluster full sync** is only used when (a) joining a fresh cluster
or (b) detecting compaction past our high-water for any origin. It
downloads the whole cluster.db snapshot from the chosen peer. This is
safe under the new design: full-sync replaces only the peer-relay
cache. **Our authoritative `local.intent_log` and our own sharded
data are preserved.** This is the architectural improvement that
made the three-database split worth doing.

**Backoff:** start at 5 s, double on failure to 5 min cap; reset on
success. On idle (caught up across all peers and origins, config
versions match), poll every 30 s.

### What full-sync recovery actually loses now

With the previous (single-cluster.db) design, full-sync recovery
threw away our own historical writes because they lived in the same
file as peer-replayed cache. The three-database split eliminates
that concern entirely:

- `local.db` is **not swapped** during cluster full-sync. Our
  authoritative log and sharded data survive.
- `cluster.db` is swapped, but it was always regenerable.
- `config.db` follows its own gossip path independently.

The remaining loss window is precisely: `local.db` itself is
destroyed. That's a true disaster-recovery scenario (disk failure on
the node's data volume), and the user-facing recovery is "restore
from your backup of `local.db`" (which is small) or "re-crawl, accept
the index gap." Cluster peers cannot reconstruct an authoritative
local.db on our behalf — by definition, they only have whatever bits
of our log they happened to pull before the loss. The design no
longer pretends otherwise.

---

## 9. Config-leader forwarding

### Server-side: every shared-config write handler
```rust
async fn create_label_rule(state, req) -> Result<Response> {
    if state.is_leader() {
        let mut tx = state.config_db.begin_immediate().await?;
        db::label_rule::insert(&mut tx, &record)?;
        db::config_audit::append(&mut tx, &actor, "create",
                                 "label_rule", &record.rule_id, None,
                                 Some(&record))?;
        db::config_meta::bump_version(&mut tx)?;
        tx.commit().await?;
        // Trigger broadcast to peers.
        state.notify_config_changed();
        Ok(Response::ok(record))
    } else {
        match state.peers.forward_to_leader(req).await {
            Ok(resp) => Ok(resp),
            Err(LeaderUnreachable) => Err(ApiError::LeaderOffline {
                leader_node_id: state.config_meta().leader_node_id,
            }),
        }
    }
}
```

The transaction touches only config.db. No intent-log entry, no
cross-file coordination. The `notify_config_changed` call wakes the
sync task to push the new version to peers (push-on-write reduces
propagation latency below the poll interval).

`ApiError::LeaderOffline` renders as 503 in JSON and as an HTMX
flash message: *"Configuration changes require <leader-name> to be
online. Reads still work."*

### Forwarder
A small helper in `mosaicfs-server::peers`. Holds a rustls client
configured with the peer cert verifier. Forwards the request body
verbatim to the leader's same path; returns the leader's response
verbatim. The route table marks each route as `LocalOnly |
LeaderOnly | LeaderForwarded`.

### Pairing (also leader-only)
Joining a new node requires the leader because only the leader can
mint a `node_id` and write the new `peer` row.
1. Joiner generates Ed25519 keypair locally; `node_id = 0` (sentinel).
2. Joiner POSTs `/sync/pair/initiate` to any cluster peer, including
   pubkey + fingerprint.
3. The receiving peer presents the joiner's fingerprint to the
   operator; symmetrically, the joiner displays the leader's
   fingerprint for confirmation on its side.
4. After both confirm, the leader allocates `node_id = MAX + 1`,
   inserts the `config.peer` row, bumps `config_meta.version`, writes
   `config_audit`, commits — all in one config.db transaction. The
   leader then pushes the new config snapshot to the joiner and the
   joiner pulls a cluster.db snapshot.
5. Joiner persists `node_id` in `local.local_node` and proceeds.

### Leader change
A new admin route `POST /api/cluster/promote-self-to-leader` that:
1. Requires explicit confirmation (HTMX dialog with type-the-name pattern).
2. On the would-be new leader: open config.db READWRITE; in one
   transaction, bump `leader_epoch`, set `leader_node_id = self`,
   reset `version` to 1, write a `config_audit` row recording the
   promotion. Commit.
3. Push the new snapshot to peers.
4. Peers accept it because `leader_epoch` strictly increased.

If the previous leader is online and disagrees, the cluster ends up
with two snapshots at the same `leader_epoch` from different
`leader_node_id` values. Detection: a node receiving a snapshot at
its current `leader_epoch` from a different leader than the one in
its own config raises an admin notification ("split-brain detected").
Resolution: operator picks one and re-promotes. NTP-syncing the
clocks means `last_modified_at` can serve as a tiebreaker for
operator inspection but not as automatic resolution.

---

## 10. Cache invalidation replacement

Today's `_changes` watcher (`mosaicfs-server/src/start.rs:264-322`)
fans out to `LabelCache`, `AccessCache`, and `readdir_cache`.

Replacement: an in-process pub/sub channel. The writer task emits
events after each commit:
```rust
enum DbEvent {
    Inserted { entity_type: &'static str, entity_key: String },
    Updated  { entity_type: &'static str, entity_key: String },
    Deleted  { entity_type: &'static str, entity_key: String },
    ConfigReplaced { new_epoch: i64, new_version: i64 },
}
```
Subscribers filter by `entity_type` and react. `entity_key` follows
the §6 canonical-string format. Sync replay also emits these events
after a batch commits, so peer-driven changes invalidate caches the
same way. `ConfigReplaced` fires after a config.db swap — caches that
mirror config (e.g., `LabelCache`) reload from the new snapshot.

The `_conflicts` notification path is removed entirely — there are no
conflicts under additive sync.

---

## 11. Schema cleanups (forced by the three-database split)

Two existing data-model decisions become untenable under config.db
and are improved as a result:

### `filesystem.availability` → `node_runtime.filesystem_mounts`

**Before.** `filesystem.availability` was a JSON aggregate (per-node
mount status) updated by the leader from heartbeats. Under config.db,
every heartbeat would force a leader write → version bump →
cluster-wide snapshot. Catastrophic.

**After.** Mount status is observed per-node and stored on
`node_runtime` as `filesystem_mounts` (JSON array of `{filesystem_id,
mount_path, mount_state, observed_at}`). It rides the existing
node-sharded sync path at heartbeat rate. The UI computes
"availability of filesystem F" by scanning `node_runtime_view` for
rows with `filesystem_mounts[*].filesystem_id == F`.

This was always the right model — mount availability is observation,
not configuration.

### `credential.last_seen` → `local.credential_observed` + `cluster.credential_observed`

**Before.** `credential.last_seen` was updated on every authenticated
request, with the design noting it as a "local write exception" that
skipped the intent log. Under config.db, even local writes to
read-only config are forbidden.

**After.** `last_seen` (and a new `request_count`) live in
`credential_observed`, keyed by `(access_key_id, observer_node_id)`.
Each node updates its own observation row and gossips it via the
intent log. UI shows "last seen on node X at T" by querying the
union view. The `credential` row in config.db carries only the
configured fields.

The old design had to handwave the `last_seen` exception; the new
schema makes the per-observer semantics explicit, which is what the
data actually was anyway.

---

## 12. UI changes (deferred to Part 021 implementation)

Listed here so design-notes captures the surface area:

- **Replace** `desktop/ui/setup.html` with a create-or-join screen.
  Equivalent web-UI bootstrap added at `/ui/bootstrap`.
- **Update** `templates/status_panel.html` to show "Database: SQLite
  (3 files)" with per-file status (size, last sync) instead of CouchDB.
- **Update** `templates/settings_backup.html` to describe per-file
  backup recommendations: `mosaicfs-local.db` is the irreplaceable
  one; `mosaicfs-cluster.db` and `mosaicfs-config.db` regenerate from
  peers.
- **Add** `templates/cluster.html` for peer list + pairing UX, with
  the leader/version status surfaced.
- **Add** flash-message rendering for `ApiError::LeaderOffline`
  across all shared-config edit forms.
- **Add** an audit-log view fed from `config_audit`.
- **Remove** the `/db/{*path}` proxy route and any UI links to it.

---

## 13. Open implementation choices for Part 018

These do not require architectural sign-off; flagging them so the
implementer knows where judgment calls live.

- **`rusqlite` vs `sqlx`.** Existing code uses `rusqlite`. Stay with
  it. The writer task is single-threaded;
  `tokio::task::spawn_blocking` around `rusqlite` calls is fine.
- **Migrations framework.** Three databases each with their own
  `schema_meta.schema_version`. Hand-rolled migration runs the
  matching upgrade scripts at startup. Reach for a proper migrations
  crate when v2 lands.
- **In-memory DBs for tests.** `Connection::open_in_memory()` and
  ATTACH multiple in-memory databases by name. The Part 017
  testsuite includes file-backed cases too because some behaviors
  (atomic swap, WAL checkpoint) only manifest with real files.
- **Connection pool sizing.** Single writer connection. Read pool
  starts at 4. Tune later.
- **Read-only enforcement on config.db.** Open with
  `SQLITE_OPEN_READONLY` on non-leaders. On leader change, close and
  reopen with `READWRITE`. Verify the leader transition path doesn't
  drop in-flight reads.
- **Empty `main` schema enforcement.** §1 specifies opening
  `:memory:` as `main` so unqualified table references fail. Verify
  in Part 017 that this works with `rusqlite`'s pool/connection
  reuse — every connection that the read pool hands out must arrive
  pre-ATTACHed. A connection-init callback in the pool builder is
  the natural hook.
