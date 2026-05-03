# Architecture Decision: Database Layer Migration (CouchDB → SQLite)

## Context

MosaicFS is a Rust-based "filesystem of filesystems" that creates a unified virtual namespace across multiple storage locations. It runs as a distributed agent on multiple nodes (e.g., NAS + laptop) that coordinate metadata about files each node has indexed.

The current architecture uses CouchDB as the coordination and metadata store. This session evaluated replacing CouchDB due to operational complexity — specifically, CouchDB has no standard Linux distribution package and requires third-party package builds.

## Decision

**Replace CouchDB with SQLite.** Each node runs its own SQLite database. Nodes sync with each other via a peer-to-peer exchange protocol implemented in the MosaicFS API layer. No separate database process is required.

## Key Architectural Insights That Justify This Decision

**The file index is naturally sharded by node.** Each node only indexes files in its own watch paths. A file record is owned exclusively by the node that discovered it. Peers receive these records as read-only replicas. This means the vast majority of data (the file index) requires no conflict resolution — sync is purely additive.

**Configuration data is small and infrequently written.** The remaining shared state (peer registry, filesystem labels, user settings) will be maintained as a single versioned JSON document in SQLite, merged using a custom field-level merge function on reconnect. Last-write-wins with timestamp comparison is sufficient given that a single human operator is making configuration changes.

**Offline writes are a real requirement.** The file watcher runs continuously and must index changes regardless of whether peers are reachable. This was the key constraint that ruled out a primary/replica model (PostgreSQL) — a node must be able to write metadata while disconnected.

## Sync Protocol Design

The intent log is the unit of replication, not the file index table itself. The file index is a derived table — it can be rebuilt at any time by replaying the intent log from the beginning.

The sync layer has three operations:

**1. Incremental sync (normal operation)**
Peers exchange intent log entries using a high-water mark protocol. On connect: "here is my sequence number per node" → "here are all my log entries since your high-water mark" → receiver replays entries in sequence order against its local derived tables. Fast and cheap, the common case.

**2. Full sync (new node onboarding and recovery)**
A node can request a complete snapshot of another node's current file index state. After receiving the snapshot it switches to incremental log exchange from that point forward. Used for:
- New node joining the cluster — never needs historical log, starts from snapshot
- Node reconnecting after the origin has compacted past its high-water mark — detects the gap, falls back to full sync automatically

**3. Intent log compaction (local, uncoordinated)**
Each node compacts its own intent log independently on an age or size basis. No coordination with peers is required because any peer that falls behind can always recover via full sync. Decommissioned nodes that never reconnect do not block compaction.

**Deletion handling**
Deletions are captured as `file_deleted` events in the intent log. Peers replay the event and issue a hard `DELETE` against their local derived table. No tombstone table, no `deleted_at` column, no special casing required.

**Conflict resolution**
Not required for the file index — each record is owned by exactly one node. The config JSON document uses field-level last-write-wins merge, comparing `updated_at` timestamps per field.

## Write Architecture Within the Agent Process

The indexer and API server run in the same process, so multi-process SQLite contention is not a concern. Within the process, a dedicated writer task owns all SQLite writes. The indexer and API handler submit write requests to it. A `sqlx` connection pool with pool size 1 for writes enforces serialization naturally. Reads use a separate larger pool. The UI talks to the agent via the API layer and never opens the SQLite file directly.

## Security Hardening — No Regressions

**Landlock:** The agent's landlock policy must explicitly permit read/write access to the SQLite database file path. If the database lives under the agent's existing permitted data directory (e.g., `/var/lib/mosaicfs/`) it may already be covered. Audit the policy to confirm the database path is included. The path must be known before the landlock policy is applied, so it must be read from config prior to policy construction.

**Seccomp:** SQLite file I/O syscalls (`read`, `write`, `fsync`, `flock`) are almost certainly already permitted. SQLite WAL mode uses `mmap` — verify `mmap` is in the seccomp allowlist.

**W^X memory protection (`MemoryDenyWriteExecute=true`):** SQLite uses an interpreted bytecode VM (VDBE) and does not JIT-compile queries. No writable-executable memory pages are required. The systemd W^X restriction remains intact without modification. Note: this assumes no SQLite extensions are loaded at runtime — the extension mechanism uses `dlopen` which may require a W^X exception, but no extensions are used in this architecture.

## First-Run Initialization UI

The desktop app and web UI must present an initialization screen on first launch, before the normal interface is shown. This replaces any existing CouchDB connection/setup UI elements, which must be removed as a prerequisite.

The initialization screen presents two options:

**1. Create a new MosaicFS node**
This node becomes the first peer in a new filesystem. A unique node ID is generated and persisted at this point. The agent proceeds directly to normal operation.

**2. Connect to an existing MosaicFS node**
The user provides the hostname or IP address and port of an existing peer. The agent initiates a full sync from that peer, receiving a complete snapshot of its file index. A progress indicator is shown during sync. On completion the node transitions automatically into normal incremental sync mode.

**Implementation notes:**
- Peer discovery is manual entry for v1. Automatic discovery via mDNS/Bonjour is a future enhancement.
- The initialization state must be persisted so subsequent UI loads skip this screen and go directly to the normal interface.
- The choice of node identity (node ID) established here is permanent and referenced throughout the sync protocol — it must not change after initialization.
- Existing UI elements related to CouchDB configuration and connection management must be identified and removed before this screen is implemented.

## What Replaces What

| Previous | Replacement | Notes |
|---|---|---|
| CouchDB | SQLite (per node) | No separate process |
| CouchDB replication | Intent log peer exchange | Events replicated, not table rows |
| CouchDB MVCC | Not needed | Data is node-sharded |
| CouchDB config docs | Versioned JSON document in SQLite | Field-level merge on sync |
| redb (v2 plan) | Dropped | SQLite handles both roles |

## Operational Benefits

- Single binary deployment story is preserved
- No separate database process to manage, package, or monitor
- Backup is a point-in-time SQLite file copy (`VACUUM INTO` or file copy with agent stopped) — this likely simplifies Phase 8 (Backup/Restore) considerably
- SQLite is available everywhere without packaging concerns

## Risks

**Sync layer complexity** is the main risk. The natural data sharding and intent log design significantly reduce scope, but the protocol still needs explicit design before implementation — particularly around full sync atomicity, partial sync failure recovery, and compaction timing.

**SQLite tooling** — CouchDB offered a browser-accessible HTTP inspection interface. Administrators should use the `sqlite3` CLI for direct database inspection when troubleshooting. A UI-based admin query interface is deferred to a future release.

## Recommended Next Steps for Claude Code Session

1. **Audit the current schema** — categorize every CouchDB document type as either node-sharded (additive sync) or shared config (JSON merge). This determines the full scope of the sync protocol.
2. **Remove CouchDB UI elements** — identify and remove all CouchDB connection and configuration UI before implementing the first-run initialization screen.
3. **Design the intent log schema** — operation type, entity type, entity id, payload, sequence number, node id, timestamp.
4. **Design the sync API endpoints** — peer discovery, high-water mark exchange, incremental delta, full sync snapshot.
5. **Implement first-run initialization UI** — node creation vs. full sync from existing peer.
6. **Audit landlock policy** — confirm the SQLite database file path is covered.
7. **Audit seccomp allowlist** — confirm `mmap` is present for SQLite WAL mode.
8. **Migration plan** — CouchDB → SQLite migration for any existing dev data.
9. **Drop redb from roadmap** — remove references from change documents and architecture docs.
