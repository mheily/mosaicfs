# Change 016: Replace CouchDB with SQLite + Custom Peer-to-Peer Sync

> **Multi-part change.** This directory holds the umbrella architecture.
> Implementation is split across `docs/changes/{016,017,018,019,020}/` per
> the project's "one moving part at a time" rule. Each numbered part below
> will get its own `architecture.md` and `design-notes.md`. Each part is
> intentionally not deployable on its own — the system runs CouchDB until
> part 020 lands, then runs SQLite. There is no dual-write transition.

Companion documents:
- `intent.md` — motivation and goals
- `discussion.md` — initial high-level proposal
- `couchdb-is-not-settled.md` for the feasibility analysis

---

## Current State Summary

_Inventory verified against commit `e44e1de` on 2026-05-02._

### Workspace
- `mosaicfs-common` (3,508 LOC) — shared types, including `couchdb.rs` (442 LOC) and `documents.rs` (1,036 LOC, ~28 document structs)
- `mosaicfs-agent` (3,125 LOC) — crawler, replication subsystem
- `mosaicfs-server` (9,580 LOC) — REST API (91 routes) + Tera/HTMX UI (48 routes, 21 templates)
- `mosaicfs-vfs` (3,176 LOC) — FUSE layer
- `mosaicfs` (binary, 436 LOC)
- `desktop` — Tauri app (separate Cargo workspace)

### CouchDB integration today

CouchDB is the single shared metadata store. Per the existing topology:

- **`mosaicfs-common/src/couchdb.rs`** (442 LOC): canonical HTTP client with `get_document`, `put_document`, `bulk_docs`, `_changes`, `_all_docs`, `_find`, `delete_document`, `db_info`, `delete_db`, `create_indexes`. 7 Mango indexes are created at boot.
- **`mosaicfs-common/src/documents.rs`** (1,036 LOC): defines the `CouchDoc<T>` envelope (`_id`, `_rev`, body) and ~28 document types — file, virtual directory, node, filesystem, credential, agent status, utilization, label assignment, label rule, plugin, annotation, notification, storage backend, replication rule, replica, access.
- **`mosaicfs-agent/src/replication.rs`** (87 LOC): configures CouchDB-to-CouchDB **continuous push/pull** replication between this node's local CouchDB and a designated "control plane" CouchDB. This is how nodes federate today.
- **`mosaicfs-server/src/start.rs:264-322`** (`changes_feed_watcher`): polls `_changes` every 2s with `since=last_seq`, dispatches updates to `LabelCache`, `AccessCache`, and `readdir_cache`. Also surfaces `_conflicts` as user notifications.
- **`mosaicfs-server/src/handlers/agent.rs`**: `/db/{*path}` route proxies CouchDB traffic for browser-side flows.
- **`mosaicfs-server/src/ui/views.rs:52-57`** and **`templates/status_panel.html`**: surface `couch_status` in the admin status panel.
- **`templates/settings_backup.html`**: copy describes "Backups dump CouchDB documents as JSON."
- **`desktop/ui/setup.html`** (180 LOC): the desktop app's first-run screen asks for CouchDB URL/user/password, calls `test_connection` then `save_settings`.
- **Call sites:** ~80 references to `CouchClient` / `couchdb::` / `CouchDoc` across ~21 files in `mosaicfs-server/handlers/`, `mosaicfs-agent/`, `mosaicfs-vfs/`, and `mosaicfs-common/`.

### SQLite already in use

- **`mosaicfs-vfs/src/cache.rs`** (495 LOC, `rusqlite 0.32` bundled): file/block cache index at `cache/index.db`. Schema: `cache_entries` table with secondary indexes. This is local-only, not replicated.
- **`mosaicfs-agent/src/replication_subsystem.rs`**: persists replication subsystem state (`replication.db`).

`rusqlite = { version = "0.32", features = ["bundled"] }` is already a workspace dependency, so adding more SQLite usage is a no-cost step from a packaging perspective.

### Sandboxing
- **`mosaicfs/src/sandbox.rs`** + **`mosaicfs/tests/sandbox_linux.rs`**: landlock + seccomp policy.
- **`deploy/systemd/mosaicfs.service:51`**: `MemoryDenyWriteExecute=yes`.

### Deployment
- **`deploy/mosaicfs.yaml`**: pod with two containers — `couchdb` (`docker.io/couchdb:3`, port 5984) and `mosaicfs` (`localhost/mosaicfs:latest`, port 8443). Two PVCs: `couchdb-data` (10 GiB), `mosaicfs-state` (1 GiB).
- **`Dockerfile.mosaicfs`**: builds the unified `mosaicfs` binary.
- **`deploy/systemd/mosaicfs.example.toml`**: TOML config including `[couchdb]` section.

### Tests
- 10 integration test scripts in `tests/integration/` driven by `tests/docker-compose.integration.yml` (CouchDB-backed).

### Already settled (from `intent.md` discussion)

- SQLite per node is the local store.
- File index replicates via append-only intent log; no conflict resolution.
- A single "config-leader" node owns shared config; other nodes proxy edits via the API. UI shows clear error when leader is offline.
- Peer auth via Ed25519 + TOFU pairing + rustls (custom cert verifier against stored peer pubkeys).
- Full-sync (clone snapshot from a peer) is the recovery safety net.
- redb is dropped from the roadmap.
- The unified-binary direction (decisions doc) is unchanged; this work happens inside `mosaicfs-server` + `mosaicfs-agent` + `mosaicfs-common` as they exist today.

---

## Goal

Replace CouchDB with per-node SQLite plus a hand-rolled peer-to-peer sync
protocol, eliminating the separate database process and giving MosaicFS
direct control over how its data federates.

---

## Changes

### 1. Storage substrate: CouchDB → SQLite (per node)

**Today.** Each node talks to a CouchDB instance over HTTP via `CouchClient`.
Documents are JSON blobs with `_id` / `_rev` envelopes. Federation happens
inside CouchDB (`_replicator` continuous push/pull to a control plane
CouchDB).

**Proposed.** Each node owns a SQLite database at `${data_dir}/mosaicfs.db`.
Tables are normalized SQL — derived from the existing document structs but
without the `CouchDoc` envelope. A new `mosaicfs-common::db` module
introduces a typed query API. The agent and server share the same
`Connection`/pool inside the unified process. The VFS file/block cache
(`mosaicfs-vfs/src/cache.rs`) remains a separate SQLite database — it is
local-only and pre-existed this change.

**Justification.** Removes the only out-of-process dependency, enabling a
single-binary deployment story. SQLite is universally packaged (no Debian
gap). Schema becomes inspectable with the standard `sqlite3` CLI. Tests
get in-memory databases for free.

### 2. Federation: CouchDB replication → intent-log sync

**Today.** `mosaicfs-agent/src/replication.rs` writes push/pull docs to
`/_replicator`. CouchDB does the actual replication continuously over HTTP.
Conflict resolution is MVCC with tombstones; `_conflicts` are surfaced as
user notifications (`mosaicfs-server/src/start.rs:291-311`).

**Proposed.** Introduce an append-only `intent_log` table in each node's
SQLite DB. Every mutation to a node-owned record (file, replica,
per-node telemetry, etc.) writes one log row in the same transaction as
the derived-table update. Each row carries `(origin_node_id, sequence_no,
op_type, entity_type, entity_id, payload, timestamp)`. A new sync service
inside the unified binary exposes HTTP endpoints (`/sync/highwater`,
`/sync/log?since=...`, `/sync/snapshot`) and a sync client task that
contacts known peers, exchanges high-water marks, pulls deltas, and
replays them against local derived tables. Per-origin sequence numbers
make replay naturally idempotent.

**Justification.** Aligns the replication mechanism with the actual data
shape: file index records have a single owner (the indexing node), so
"additive log shipping" is sufficient — no MVCC, no conflict UI. Removes
the second CouchDB instance (control plane) from the topology.

### 3. Shared config: documents-as-data → single config-leader

**Today.** Shared configuration (virtual directories, label rules,
storage backends, replication rules, plugin configs, filesystem
definitions) lives as ordinary CouchDB documents. Any node can edit;
CouchDB's MVCC handles conflicts but produces `_conflicts` warnings the
user must resolve.

**Proposed.** One node holds the **config-leader** role, recorded in a
`cluster_meta` table replicated to all peers via the intent log. All
write APIs that mutate shared config check leader status: if the local
node is the leader, write locally and append to the intent log; otherwise
forward the request to the leader over the existing API (using paired
peer auth — see change 5). When the leader is unreachable, the UI returns
a clear error: *"Configuration changes require <leader-node> to be
online. <Reads still work.>"* A manual "promote this node to leader"
admin action exists for permanent leader loss; it requires the user to
confirm and is recorded in the intent log.

**Justification.** Avoids LWW silently dropping fields. Avoids vector
clocks and conflict-resolution UI. Aligns with the project's
single-operator audience. Failure mode is loud and clear (UI error)
rather than silent data loss.

### 4. First-run initialization: CouchDB connection prompt → cluster join/create

**Today.** Desktop: `desktop/ui/setup.html` prompts for CouchDB URL +
admin/password. Server: assumes CouchDB is reachable at boot and exits if
not.

**Proposed.** Both web UI and desktop present a first-run screen with
two options:
1. **Create a new MosaicFS cluster.** Generates a node ID, generates an
   Ed25519 keypair, marks this node as the founding config-leader,
   initializes the SQLite schema, and proceeds to normal operation.
2. **Join an existing cluster.** User enters peer hostname + port and the
   short fingerprint shown by that peer. Performs TOFU pairing, then
   initiates a full-sync from that peer. Progress UI during sync. On
   completion, transitions to incremental sync and normal operation.

The desktop's existing `setup.html` is replaced (not extended) — the
CouchDB URL/user/password fields, `test_connection`, and `save_settings`
flows are removed.

**Justification.** The user no longer thinks about a database; the
question becomes "is this a new cluster or am I adding to an existing
one?" This matches the new mental model where MosaicFS is a self-contained
peer-to-peer system.

### 5. Peer authentication: shared CouchDB credentials → Ed25519 + TOFU + rustls

**Today.** Cross-node trust is implicit in shared CouchDB credentials
(`COUCHDB_USER` / `COUCHDB_PASSWORD`). Anyone with the CouchDB password
can read/write everything. The HTTP API (`mosaicfs-server`) has its own
HMAC auth (`mosaicfs-server/src/auth/hmac_auth.rs`) for end-user requests
but is not used for peer-to-peer flows.

**Proposed.** Each node generates an Ed25519 keypair on first run, stored
in the SQLite DB (or macOS Keychain when available). Pairing displays a
short fingerprint derived from the public key; the operator confirms it
out-of-band. Each peer persists the other's pubkey. Sync endpoints
(`/sync/*`) and forwarded config-leader writes use HTTPS with a custom
rustls certificate verifier that checks the presented cert chain against
the stored peer-pubkey set. Self-signed certs derived from the node
keypair (no public CA). Authorization granularity is cluster-wide: any
paired peer can read or pull anything.

**Justification.** Removes the shared-secret model. Each node has a
unique identity. Compromise of one node does not silently authorize the
attacker to impersonate another. Trust model matches the user's mental
model ("these are my devices").

### 6. Sandbox policy

**Today.** Landlock policy permits the existing data directory (cache
`index.db` is already covered). Seccomp allowlist permits the syscalls
needed by `rusqlite` cache code. `MemoryDenyWriteExecute=yes` in the
systemd unit.

**Proposed.** Verify (and extend if needed) the landlock policy in
`mosaicfs/src/sandbox.rs` covers the new `mosaicfs.db` path. Verify
seccomp allowlist permits `mmap` (SQLite WAL mode uses memory-mapped
I/O) — likely already present from the cache code. W^X stays on; SQLite's
VDBE is interpreted, no JIT, and no extension loading happens at runtime.

**Justification.** No regressions. Relevant to acknowledge but expected
to be a small audit, not a design change.

### 7. Deployment manifest

**Today.** `deploy/mosaicfs.yaml` runs two containers (CouchDB +
mosaicfs) and two PVCs.

**Proposed.** Drop the CouchDB container and the `couchdb-data` PVC.
Resize `mosaicfs-state` PVC to absorb the metadata DB (10 GiB total
should be plenty for the foreseeable future). Update
`deploy/systemd/mosaicfs.example.toml` to remove the `[couchdb]`
section. Update `Dockerfile.mosaicfs` if it bakes in CouchDB readiness
checks.

**Justification.** Single container is part of the goal.

---

## Implementation Phases (multi-part change)

Each phase becomes its own numbered change directory. Cross-phase
dependencies are listed; intermediate states are not expected to ship.

### Part 016 — Architecture umbrella (this document)
Schema audit, intent log design, sync protocol design, and the umbrella
plan. **Deliverable:** this `architecture.md` plus a `design-notes.md`
that finalizes the schema mapping (every document type categorized as
either node-sharded or shared-config), the intent log row format, and
the sync protocol wire format. No code in this part.

### Part 017 — SQLite storage layer
Implement the new `mosaicfs.db` schema and a typed query API in
`mosaicfs-common::db` covering all current document types. Wire the
unified process to use it as the **only** metadata store — CouchDB
reads/writes are removed in this phase. The intent log is populated but
not yet exchanged. The continuous-replication setup
(`mosaicfs-agent/src/replication.rs`) and the `_changes` watcher
(`mosaicfs-server/src/start.rs`) are deleted; cache invalidations move
to direct in-process notifications. **Deliverable:** the system runs as
a single-node store on SQLite. Multi-node scenarios are broken until
part 018 lands.

### Part 018 — Sync protocol
Add `/sync/*` HTTP endpoints, the sync client task, full-sync snapshot
generation and restore, intent-log compaction. Includes a fault-injection
test harness (two-node sim with controllable partitions, replay,
crash-mid-replay, compaction-during-snapshot). **Deliverable:** two nodes
can federate. Auth is still trusted-LAN only; secured in part 019.

### Part 019 — Peer auth (Ed25519 + TOFU + rustls)
Generate per-node keypair, implement TOFU pairing UX (fingerprint
display + entry), add custom rustls cert verifier. Sync endpoints reject
unpaired peers. Config-leader forwarding uses paired auth.
**Deliverable:** cross-LAN sync is safe.

### Part 020 — Init UX, deployment, doc cleanup
Replace `desktop/ui/setup.html` with create-or-join screens. Add
equivalent web-UI bootstrap flow. Update `deploy/mosaicfs.yaml`
(drop CouchDB container) and systemd example TOML. Update
`templates/settings_backup.html` copy and `templates/status_panel.html`.
Remove `/db/*` proxy route. Update `.claude/skills/decisions/SKILL.md`
to remove "CouchDB stays." Delete `mosaicfs-common/src/couchdb.rs` and
`mosaicfs-agent/src/replication.rs`.
**Deliverable:** the change is complete; CouchDB is gone from the tree.

---

## What Does Not Change

- **Workspace layout**: same five crates, same boundaries. The unified-binary
  direction proceeds independently of this change.
- **REST API surface**: the 91 existing routes mostly stay. The CouchDB-proxy
  route `/db/{*path}` is removed (part 020). Sync routes `/sync/*` are added
  (part 018). Some handler internals change to use the new `db` module
  instead of `CouchClient`, but URL paths and request/response shapes are
  preserved where possible.
- **UI framework**: Tera + HTMX (per decisions doc). 21 existing templates
  largely unchanged; only the bootstrap/init flow and a handful of admin
  panels (status, backup) get updates.
- **Desktop app architecture**: Tauri app stays. Only the setup screen and
  the underlying connection logic change.
- **VFS layer**: `mosaicfs-vfs` continues to use its own SQLite cache
  (`cache.rs`). Its read path against the metadata store changes from
  `CouchClient::get_document` to typed `db` calls, but the FUSE-facing
  contract is unchanged.
- **Auth for end-user requests**: HMAC auth in
  `mosaicfs-server/src/auth/hmac_auth.rs` continues unchanged.
- **Sandbox model**: landlock + seccomp + W^X stay; the policy is audited
  and minimally adjusted, not redesigned.
- **Storage backends**: the storage-backends UI page and its actions remain
  (the page still lets operators manage backend documents in CouchDB).
  The replication subsystem and its Rust code have been removed (see
  `docs/future/replication.md`).
- **Document semantics**: the existing document structs in
  `documents.rs` map directly to SQL tables; field names and meanings
  stay the same. Only the storage representation changes.

---

## Deferred

- **mDNS / Bonjour peer discovery.** Manual hostname entry for v1; auto-discovery is a UX nicety, not a correctness requirement.
- **Web admin UI for SQLite** (Fauxton replacement). Use the `sqlite3` CLI for direct inspection during dev/debug. A web SQL console can come later if the operational pain is real.
- **FTS5 search integration.** Listed in `intent.md` as a goal but not required for parity. Add as a separate change once the storage migration lands.
- **PostgreSQL portability** ("real database server" path). The intent doc raises this as a possible future direction. Note that the *sync protocol* layered on top of SQLite would not port to Postgres trivially, so this benefit applies to SQL queries, not to the federation layer.
- **Per-resource ACLs** for paired peers. v1 trust granularity is cluster-wide ("any paired peer can read anything"). Finer-grained authorization can be added if needed by a real-world use case.
- **Vector-clock-based config conflict resolution.** Replaced with single config-leader. If multi-leader config editing becomes necessary, revisit then.
- **NAT traversal for cross-internet sync.** Document that remote-access scenarios should ride a VPN (Tailscale, WireGuard) for v1.
- **Unified binary consolidation.** Tracked separately in the decisions doc as ongoing work; not entangled with this change.

---

## Resolved Decisions

These were open questions during architecture drafting; the developer
has confirmed the resolutions below. They are inputs to part 016's
`design-notes.md`.

1. **Schema categorization.** Each document type is classified as either
   node-sharded or shared-config:
   - **Node-sharded (intent-log additive sync):** `FileDocument`,
     `AgentStatusDocument`, `UtilizationSnapshotDocument`,
     `AccessDocument`, `NotificationDocument` (per-node observation),
     and the node-owned half of the split `NodeDocument` (see below).
   - **Shared config (config-leader writes):** `VirtualDirectoryDocument`,
     `FilesystemDocument`, `LabelRuleDocument`, `LabelAssignmentDocument`,
     `CredentialDocument`, and the user-owned half of the split
     `NodeDocument`.
   - **Removed before change 016** (see `docs/future/` for recovery):
     `PluginDocument`, `AnnotationDocument`, `ReplicaDocument`,
     `ReplicationRuleDocument`, `StorageBackendDocument`.
   - **`NodeDocument` is split.** Today's single struct mixes
     node-owned runtime fields (e.g., `status`, `last_heartbeat`) with
     user-owned config fields (e.g., `friendly_name`). These become two
     distinct entities in the new schema: a `NodeRuntime` record
     (node-sharded, written by the node itself, replicated via intent
     log) and a `NodeConfig` record (shared-config, written via the
     config-leader). Field-by-field split is finalized in
     `design-notes.md`.
2. **Intent log row format.** `payload` stores the full record. Simpler
   replay code wins over storage savings. Diff-based payloads are
   deferred unless on-disk size becomes a real problem.
3. **Compaction policy.** Default thresholds: 30 days age, 100 MB size.
   Both configurable via TOML.
4. **Sync wire format.** JSON over HTTP. Tooling simplicity over
   throughput. Revisit only if profiling shows it's a bottleneck.
5. **Snapshot format for full-sync.** SQLite backup file (via
   `VACUUM INTO` or the backup API) plus a small JSON manifest carrying
   the snapshot's anchor sequence numbers per origin node. Receiver
   imports the file atomically, then resumes incremental sync from the
   manifest's anchor.
6. **Two pre-existing clusters trying to pair.** v1 rejects the join
   with a clear error directing the user to reconfigure one of them as
   a fresh single-node cluster (i.e., wipe its state and re-join).
   Cluster merge is not supported. The leader is the only entry point
   for joins (it allocates `node_id` and writes the new `peer` row as
   shared config), so this check lives in one place: the leader
   refuses any pair request from a node whose own `peer` table is
   non-empty.
7. **HMAC auth interaction with peer auth.** The two layers apply to
   disjoint route sets (HMAC on existing `/api/*`, peer auth on new
   `/sync/*` and the config-leader forwarding path). Verifiers are
   independent. To be confirmed in `design-notes.md` once the route
   table is finalized.
