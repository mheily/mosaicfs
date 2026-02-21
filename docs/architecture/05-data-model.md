<\!-- MosaicFS Architecture · ../architecture.md -->

## PART TWO — Technical Reference

This section contains the detailed technical specifications for the MosaicFS system: document schemas, data structures, protocols, and component interfaces. It is intended as a reference for implementors.

---

## Technology Stack

| Component | Technology | Notes |
|---|---|---|
| Agent daemon | Rust | Single static binary. Uses tokio for async, notify crate for filesystem watching. |
| VFS common layer | Rust (`mosaicfs-vfs`) | Shared library crate. Rule evaluation, tiered access, file cache (full-file and block modes), download deduplication. Used by all OS-specific backends. |
| FUSE backend (v1) | Rust / fuser | Implemented within the agent binary. Uses the `fuser` crate for FUSE bindings. Read-only in v1. Used on Linux and macOS (via macFUSE). |
| macOS File Provider (future) | Swift / FileProvider framework | Separate macOS app extension communicating with the agent via XPC. Provides native Finder integration, on-demand hydration, sync-state badges. |
| Windows CFAPI (future) | Rust / Windows crate | Desktop app component alongside the agent. Uses the Windows Cloud Files API (`cfapi.h`). Provides native File Explorer integration, placeholder files, hydration progress UI. |
| GIO / KIO backends (future) | C / Rust FFI | GVfs backend (GNOME) and KIO worker (KDE). Registers `mosaicfs://` URI scheme for desktop-aware applications. Calls the REST API or agent Unix socket; no kernel driver required. |
| Control plane API | Rust / Axum | Built on tokio + hyper. Serves both the REST API and static web UI assets. |
| Database | CouchDB 3 | Runs in Docker on the control plane host. Never exposed externally. |
| Agent local DB | Rust CouchDB client | Speaks the CouchDB replication protocol natively. |
| Web UI | React + Vite | Single-page application. Uses shadcn/ui components, TanStack Query for API calls. |
| Browser sync | PouchDB | Syncs directly with CouchDB as the `mosaicfs_browser` read-only user. Session token issued by Axum at login. Pull-only; push rejected at database level. |
| Deployment | Docker Compose | Control plane runs as a Compose stack. Agents install as systemd / launchd services. |

---

## Data Model Overview

All state in MosaicFS is stored as JSON documents in CouchDB and replicated between agents and the control plane. There are no separate relational tables or sidecar databases for core metadata — everything lives in one document store, which is what makes the replication model so clean. Understanding the document types and how they relate to each other is the key to understanding how the system works.

**Atomicity model.** CouchDB does not provide multi-document transactions. Each document write is atomic in isolation, but `_bulk_docs` batches are not transactional — individual documents in a batch can succeed or fail independently. MosaicFS is designed around this constraint: no operation requires atomically updating two documents simultaneously. When related state spans multiple documents (e.g. a file document and its label assignment), the system tolerates temporary inconsistency — the rule engine and search API will see one update before the other, which produces correct (if briefly incomplete) results. The SQLite sidecar databases (`cache/index.db`, `plugin_jobs.db`) do use transactions internally for their own consistency, but these are local to the agent and not replicated.

### Document Types at a Glance

MosaicFS uses fifteen document types in v1, each with a distinct role in the system. Two additional types — `peering_agreement` and `federated_import` — are designed but not implemented; they are described in the Federation section.

| Document Type | `_id` Prefix | Purpose |
|---|---|---|
| `file` | `file::` | One document per indexed file. The core unit of the system. File identity is location-independent — the `_id` is a UUID with no embedded node reference. The file's current location (`source.node_id`, `source.export_path`) is stored as mutable fields, enabling migration between nodes without changing identity. Virtual locations are computed on demand by the rule engine; not stored on the document. |
| `virtual_directory` | `dir::` | One document per directory in the virtual namespace. Explicitly created and managed by the user. Carries the directory's mount sources — the rules that define what files and subdirectories appear inside it. |
| `node` | `node::` | One document per participating device. Describes the node, its transfer endpoint, storage topology, and embedded network mount declarations. |
| `credential` | `credential::` | Preshared access key pairs used by agents and the web UI to authenticate with the control plane. |
| `agent_status` | `status::` | Published periodically by each agent. Provides operational health data for the web UI dashboard. |
| `utilization_snapshot` | `utilization::` | Point-in-time record of storage capacity and usage for a node. Written hourly; used to compute utilization trends over time. |
| `label_assignment` | `label_file::` | Associates one or more user-defined labels with a specific file, identified by file UUID. Labels attach to the file's identity, not to a specific copy on a specific node — they survive migration without changes. Written by the user via the API; never touched by the agent crawler. |
| `label_rule` | `label_rule::` | Applies one or more labels to all files under a given path prefix on a given node. Acts as an inherited label source: a file's effective label set is the union of its direct `label_assignment` labels and all `label_rule` labels whose prefix covers the file's `export_path`. |
| `plugin` | `plugin::` | Configuration for one plugin on one node. Specifies the plugin type (`executable` or `socket`), the plugin name (resolved to a binary in the node's plugin directory), subscribed events, MIME filter globs, worker count, timeout, and an arbitrary `config` object passed to the plugin at invocation time. Managed via the web UI; the agent watches the changes feed and reloads plugin configuration live. |
| `annotation` | `annotation::` | Structured metadata written back to CouchDB by executable plugins. One document per `(file, plugin_name)`, keyed by file UUID. Annotations attach to the file's identity and survive migration without changes. The plugin's entire stdout JSON object is stored verbatim in the `data` field. Socket plugins that update external systems typically produce no annotation documents. |
| `access` | `access::` | Records the most recent time a file was accessed through MosaicFS (VFS, REST API, or agent transfer). One document per file, written by the agent that served the access. Updated with debouncing to limit replication churn. Used by the `access_age` step pipeline operation. |
| `storage_backend` | `storage_backend::` | Defines an external storage service connection — backend type, credentials, mode (source/target/bidirectional), schedule, retention policy. Managed via the control plane API. Source-mode backends index files into MosaicFS; target-mode backends receive file replicas. Referenced by replication rules and replica documents. |
| `replication_rule` | `repl_rule::` | Defines which files should be replicated to which target using the step pipeline vocabulary. Evaluated by the agent's replication subsystem. Analogous to `label_rule` but for replication rather than labeling. |
| `replica` | `replica::` | Records that a copy of a specific file exists on a specific replication target. Written by the agent after a successful upload via a storage backend plugin. Read by the VFS layer for Tier 4b failover. One document per (file, target) pair. |
| `notification` | `notification::` | A system event or condition requiring user attention. Written by agents, storage backends, the control plane, and plugins. One document per distinct condition — identified by a stable `condition_key` so the same condition updates rather than duplicates. Carries severity, source, message, optional action links, and a lifecycle status (`active`, `resolved`, `acknowledged`). Replicated to the browser via PouchDB for live delivery without polling. |

### How the Document Types Relate

The relationships between document types reflect the layered architecture of the system. At the bottom is the physical layer: nodes own files on real filesystems. Above that is the virtual layer: virtual directories carry mount sources that define what files appear inside them, and the rule engine evaluates those sources on demand to answer directory listings. Connecting the two is the access layer: network mount documents let the VFS layer find the cheapest path to each file's bytes. Cutting across all layers is the label system: label assignments and label rules attach arbitrary user-defined tags to files, which the rule engine and search API can filter on.

A `file` document is a fact about a real file — where it lives and what it looks like. Its `_id` is a location-independent UUID, so the file's identity is stable across migrations between nodes. The `source.node_id` and `source.export_path` fields describe where the file currently lives; these are mutable and updated during migration. The file document has no knowledge of where it appears in the virtual tree, and it carries no labels directly. Labels are stored in separate `label_assignment` documents (keyed by file UUID, never overwritten by the crawler) and `label_rule` documents (which apply labels to entire directory subtrees by path prefix). A file's effective label set — the union of its direct assignments and all prefix-matching rules — is computed at query time by the rule engine and search API.

Access times are tracked in separate `access` documents, updated with debouncing whenever a file is accessed through MosaicFS (VFS, REST API, or agent-to-agent transfer). The file document itself does not store access time — this avoids coupling the crawler's write path to read activity and prevents replication churn from frequent file opens.

File replication is managed through three document types that mirror the label system's design. `storage_backend` documents define external storage connections (S3, B2, local directory, Google Drive, iCloud) — analogous to defining a storage pool. `replication_rule` documents define which files should be replicated to which targets using the same step pipeline operations as virtual directory mounts. `replica` documents record the fact that a specific file has been copied to a specific target — the VFS layer reads these directly for Tier 4b failover when the owning node is offline. The actual bytes-on-wire interaction with each storage service is handled by thin storage backend plugins; the replication orchestration (rule evaluation, scheduling, state tracking) is core.

`virtual_directory` documents are the primary configuration surface. A directory's `mounts` array describes what gets mounted inside it — each mount entry specifying a source, a filter step pipeline, and a mapping strategy. Directories are created and deleted explicitly by the user; they are not created automatically as a side effect of rules. An empty directory (one with no mounts) is a valid, persistent container for other directories.

### How Each Component Uses the Data Model

Different components of MosaicFS have distinct, non-overlapping write responsibilities. Understanding who writes what is important for reasoning about data consistency.

| Component | Writes | Reads |
|---|---|---|
| Agent crawler / watcher | `file`, `agent_status`, `utilization_snapshot`, `notification` (crawl events, watch limit, cache pressure) | `credential` (auth), node's `network_mounts` (path hints) |
| Agent replication subsystem | `replica` (after successful upload/deletion), `notification` (target unreachable, backlog) | `storage_backend`, `replication_rule`, `file`, `replica`, `access` (for rule evaluation), `label_assignment`, `label_rule` |
| Agent plugin runner | `annotation` (from executable plugin stdout), `agent_status` (plugin subsystem health), `notification` (job failures, plugin health check results) | `plugin` (configuration), `file` (event payloads), `annotation` (stale check on re-crawl) |
| Rule evaluation engine (VFS layer / control plane) | Nothing — read-only evaluation | `file`, `virtual_directory` (mount sources + steps), `node`, `label_assignment`, `label_rule`, `annotation`, `replica` |
| VFS backend (FUSE / File Provider / CFAPI) | `access` (debounced, via access tracker) | `file`, `virtual_directory` (readdir), node's `network_mounts`, `node`, `replica` (Tier 4b failover) |
| Control plane API (Axum) | `credential`, `node` (registration, network_mounts), `virtual_directory`, `label_assignment`, `label_rule`, `storage_backend`, `replication_rule`, `plugin`, `notification` (system-level events, credential activity), `access` (from REST API content downloads) | All document types |
| Web UI (browser / PouchDB) | `virtual_directory` (via API), node's `network_mounts` (via API), `label_assignment` (via API), `label_rule` (via API), `plugin` (via API), `notification` (acknowledge via API) | All document types (via PouchDB live sync) |

### Replication Topology

Not all documents are replicated to all nodes. The replication topology is filtered to match each node's needs:

- **Physical agents** replicate `file`, `virtual_directory`, `node`, `credential`, `label_assignment`, `label_rule`, `storage_backend`, `replication_rule`, `replica`, `plugin`, `annotation`, `access`, and `notification` documents bidirectionally with the control plane. Network mount declarations travel as part of the node document rather than as separate documents. This gives the VFS layer everything it needs to evaluate directory mount sources, resolve label sets, query annotations, and find file locations without a network round trip. Plugin configuration documents replicate to agents so the plugin runner can load them without contacting the control plane. Notification documents replicate bidirectionally so the browser receives notifications from agents in real time via PouchDB, and acknowledgements written by the browser via the REST API propagate back to the originating agent.
- **`agent_status`** is pushed from each agent to the control plane only — it is not replicated back out to other agents, since no agent needs to know the health of another agent directly.
- **The browser (PouchDB)** syncs a read-only subset of the database directly with the control plane's CouchDB instance, enabling live-updating UI without custom WebSocket infrastructure. `credential` documents are excluded from browser replication for security.

### Soft Deletes and Document Lifecycle

MosaicFS uses soft deletes for file documents rather than CouchDB's native deletion mechanism. When a file is removed from a node's filesystem, its document is updated with `status: "deleted"` and a `deleted_at` timestamp rather than being deleted outright. This preserves the inode number if the file reappears, ensures other nodes learn about the deletion through normal replication, and maintains a deletion history for debugging. Migration also produces soft deletes on the source node — the file document's `source.node_id` is updated to the destination, and the source agent marks its local copy as deleted after confirming the transfer.

Virtual directory documents are explicitly created and deleted by the user. They are never created or tombstoned automatically. Node and credential documents are never deleted; they are disabled via a `status` or `enabled` flag to preserve the audit trail and prevent orphaned references.

### CouchDB Document Conflict Resolution

CouchDB's multi-master replication model means that two nodes can update the same document concurrently, producing a conflict. CouchDB automatically picks a deterministic winner (based on revision tree depth, then lexicographic `_rev`), but the losing revision persists as a conflict marker until explicitly resolved. MosaicFS handles conflicts with a simple strategy tailored to each document type's write ownership model:

**File documents** — owned exclusively by the agent whose `node_id` matches `source.node_id`. Conflicts should not occur in normal operation because only one agent writes to a given file document. During migration, `source.node_id` is updated atomically by the control plane, transferring ownership to the destination agent. If a conflict does occur (e.g. after a network partition where the same agent reconnected with a stale checkpoint), the agent resolves it on the next crawl cycle: the crawler always writes the current filesystem state, so the latest write is authoritative. On detecting a `_conflicts` array, the agent deletes the losing revisions.

**Virtual directory documents** — written only by the control plane API in response to user actions. Conflicts are possible if two browser sessions edit the same directory simultaneously. The control plane API uses optimistic concurrency: the PUT request includes the current `_rev`, and CouchDB rejects the update if the revision has changed. The UI re-fetches and prompts the user to retry. No automatic merge is attempted.

**Node documents** — written by both the owning agent (status, heartbeat, storage, capabilities) and the control plane API (network_mounts, friendly_name). These are the most conflict-prone documents. The resolution strategy is last-write-wins using CouchDB's automatic winner selection, which is acceptable because the two writers update disjoint field sets and the agent will overwrite its fields on the next heartbeat cycle. To reduce conflict frequency, the control plane API uses `_rev`-conditional updates and the agent avoids writing unchanged fields.

**Label assignment and label rule documents** — written only via the control plane API. Same optimistic concurrency as virtual directories.

**Credential documents** — written only via the control plane API. Same optimistic concurrency.

**Plugin documents** — written only via the control plane API. Same optimistic concurrency.

**Agent status documents** — written exclusively by the owning agent. No conflicts expected.

**Utilization snapshot documents** — written exclusively by the owning agent. No conflicts expected (each snapshot has a unique timestamp-based `_id`).

**Storage backend and replication rule documents** — written only via the control plane API. Same optimistic concurrency as virtual directories.

**Replica documents** — written exclusively by the agent's replication subsystem after a successful upload or deletion. Each `(file, target)` pair is managed by a single agent (the one running the replication subsystem that targets this file). No conflicts expected in normal operation.

**Annotation documents** — written exclusively by the plugin runner on the owning agent. No conflicts expected.

**Access documents** — each file has at most one access document, written by the agent that most recently served the access. If two agents serve the same file concurrently (e.g. via VFS on two different nodes), both may update the same document. Last-write-wins is acceptable — the only consequence is a `last_access` timestamp that is off by the debounce interval. Access documents carry no critical state; losing a revision loses at most one timestamp update.

**Notification documents** — written by the source (agent, storage backend, or control plane) and updated by the control plane API (acknowledgement). Conflicts are possible if a source updates a notification while the user acknowledges it. Resolution is last-write-wins; the worst case is a re-fired notification that the user acknowledges again.

**Conflict monitoring.** The control plane runs a periodic background task (every 60 seconds) that queries for documents with `_conflicts` and logs them. Persistent conflicts that are not auto-resolved within 5 minutes generate a notification document (`notification::control_plane::persistent_conflicts`).

### CouchDB Indexes

CouchDB Mango indexes are created at setup time — on the control plane during initial setup, and on each agent at first startup. Each index covers a specific query pattern used by one or more components. Without these indexes, Mango falls back to full collection scans, which are acceptable for very small deployments but degrade as the file count grows.

The **Location** column indicates where each index must exist. "Control plane" means the index is only needed on the central CouchDB instance. "Agent local" means the index must also be created on each agent's local CouchDB replica, because the VFS layer or agent-side authentication queries that replica directly without going through the control plane.

| Index Fields | Location | Used By | Purpose |
|---|---|---|---|
| `type`, `status` | Control plane + Agent local | Search API, VFS layer | The baseline filter applied to almost every query. Narrows the candidate set to active file documents before any further filtering. |
| `type`, `source.node_id`, `source.export_path` | Control plane + Agent local | Rule engine (`readdir`), VFS layer (`open`) | Resolves a node ID and export path to its file document. Used by the rule engine when evaluating mount sources and by the VFS layer to open files. Must be local so filesystem operations require no network round trip. |
| `type`, `source.node_id`, `source.export_parent` | Control plane + Agent local | Rule engine (`readdir`) | Lists all files under a given real directory on a specific node. Used when evaluating a `prefix_replace` mount source — fetches all files whose `export_parent` starts with the source path prefix. Must be local for VFS performance. |
| `type`, `source.node_id` | Control plane only | Nodes page | Fetches all files belonging to a specific node. Used by the web UI node detail page to show indexed file counts. Not needed locally — agents don't query other nodes' files. |
| `type`, `status`, `name` | Control plane only | Search API | Supports filename substring and glob search. The `type` and `status` fields narrow the scan to active file documents; the regex match on `name` is then applied to this reduced set. Search runs on the control plane only. |
| `type`, `inode` | Control plane + Agent local | VFS layer | Resolves an inode number back to a document. Used by FUSE operations that receive an inode rather than a path. Must be local so inode resolution requires no network round trip. |
| `type`, `node_id`, `captured_at` | Control plane only | Storage page, utilization trend charts | Queries utilization snapshots for a given node over a time range. Not replicated to agents; only the control plane and web UI query snapshot history. |
| `type`, `enabled` | Control plane + Agent local | Authentication middleware, agent-to-agent transfers | Looks up a credential document during request signing validation. Must be local on agents because transfer authentication between two agents is validated against the local replica without involving the control plane. |
| `type`, `status` (node docs) | Control plane only | Dashboard, health checks | Fetches all nodes with a given status. Used by the dashboard to render node health indicators and by the control plane's health check poller. |
| `type`, `file_id` (label_assignment docs) | Control plane + Agent local | Rule engine (label step), Search API | Looks up a `label_assignment` document for a specific file by `file_id`. Used when computing a file's effective label set during step pipeline evaluation and label-based search. Must be local for VFS performance. |
| `type`, `node_id`, `path_prefix` | Control plane + Agent local | Rule engine (label step), Search API | Lists all `label_rule` documents that could cover a given file path on a given node. The rule engine loads all rules for the relevant node and checks which prefixes match. Must be local for VFS performance. |
| `type`, `file_uuid`, `plugin_name` (annotation docs) | Control plane + Agent local | Rule engine (annotation step), Search API | Looks up an `annotation` document for a specific file and plugin. Used during step pipeline evaluation for the `annotation` op and during annotation-based search. Must be local for VFS performance. |
| `type`, `file_id` (access docs) | Control plane + Agent local | Rule engine (`access_age` step), access tracker | Looks up the `access` document for a specific file by `file_id`. Used during step pipeline evaluation for the `access_age` op and by the access tracker when flushing debounced updates. Must be local for VFS performance. |
| `type`, `target_name` (storage backend docs) | Control plane + Agent local | Agent replication subsystem, REST API | Looks up a storage backend by name. Must be local so the agent can resolve backend configurations without a control plane round trip. |
| `type`, `target_name` (replication rule docs) | Control plane + Agent local | Agent replication subsystem | Fetches all replication rules for a given target. Must be local so rule evaluation runs without network round trips. |
| `type`, `enabled` (replication rule docs) | Control plane + Agent local | Agent replication subsystem | Fetches all enabled replication rules. Used during periodic re-evaluation scans and on changes feed updates. |
| `type`, `file_id` (replica docs) | Control plane + Agent local | VFS layer (Tier 4b), rule engine (`replicated` step), REST API | Looks up all replica documents for a specific file. Used by Tier 4b failover to find available replicas when the owning node is offline. Must be local for VFS performance. |
| `type`, `target_name` (replica docs) | Control plane + Agent local | Agent replication subsystem, REST API | Lists all replicas on a given target. Used for manifest reconciliation and restore operations. |
| `type`, `node_id` (plugin docs) | Control plane + Agent local | Agent plugin runner | Fetches all enabled plugin configurations for a given node. The agent loads this on startup and reloads on changes feed updates. Must be local so the plugin runner does not require a control plane round trip at startup. |
| `type`, `status`, `severity` (notification docs) | Control plane only | Notification API, dashboard | Fetches active and unacknowledged notifications sorted by severity for the notification panel and dashboard alert area. The browser receives notification documents via PouchDB live sync and filters client-side, but the REST API uses this index for server-side queries. |

A note on `$regex` queries: CouchDB Mango does not support true text indexes — `$regex` always performs a scan of the candidate set after index filtering. For filename search this means the `type` + `status` + `name` index reduces the scan to active file documents, but the regex itself is evaluated in memory on the control plane. This is acceptable at home-deployment scale. If search performance degrades as the file count grows, the correct solution is a dedicated search engine rather than further CouchDB index tuning.

### Replication Flows

CouchDB replication between agents and the control plane is filtered — each flow carries only the documents the destination actually needs. This keeps agent replicas lean and avoids leaking sensitive documents (credentials, utilization history) to nodes that have no use for them. Filters are expressed as Mango selectors attached to CouchDB replication documents.

There are three network replication flows.

**Flow 1 — Agent → Control Plane (push)**

Each agent pushes only the documents it owns or that the user has created locally. It never pushes documents it received from the control plane back upstream.

```json
{
  "$or": [
    { "type": "file",                 "source.node_id": "<this_node_id>" },
    { "type": "node",                 "_id":            "node::<this_node_id>" },
    { "type": "agent_status",         "node_id":        "<this_node_id>" },
    { "type": "utilization_snapshot", "node_id":        "<this_node_id>" },
    { "type": "annotation",           "source.node_id": "<this_node_id>" },
    { "type": "access",               "source.node_id": "<this_node_id>" },
    { "type": "replica",              "source.node_id": "<this_node_id>" },
    { "type": "notification",         "source.node_id": "<this_node_id>" }
  ]
}
```

**Flow 2 — Control Plane → Agent (pull)**

The agent pulls everything the VFS layer and local authentication need to operate without contacting the control plane. Network mount declarations travel as part of the node document. Only disabled credentials are excluded to keep the local replica clean.

```json
{
  "$or": [
    { "type": "file" },
    { "type": "virtual_directory" },
    { "type": "node" },
    { "type": "credential",       "enabled": true },
    { "type": "label_assignment" },
    { "type": "label_rule" },
    { "type": "plugin" },
    { "type": "annotation" },
    { "type": "access" },
    { "type": "storage_backend" },
    { "type": "replication_rule" },
    { "type": "replica" },
    { "type": "notification" }
  ]
}
```

The following document types are deliberately excluded from agent replicas: `agent_status` (agents don't monitor each other), `utilization_snapshot` (history only needed by the control plane and web UI). Plugin documents for *other* nodes are excluded — each agent only needs its own node's plugin configurations, which arrive via the node-scoped filter in Flow 1. Control-plane-originated notification documents (OAuth expiry, system-level alerts) replicate to agents via this flow so the browser receives them through the same PouchDB channel regardless of origin.

**Flow 3 — Control Plane → Browser (PouchDB pull)**

The browser authenticates to CouchDB as the `mosaicfs_browser` user — a restricted CouchDB role created during control plane setup. This user has read-only access to the `mosaicfs` database, enforced by CouchDB's own permission model. Push attempts from a browser client are rejected at the database level regardless of what the replication filter says — a hijacked session cannot write to the database.

The Axum login endpoint issues a short-lived CouchDB session token for `mosaicfs_browser` alongside the JWT used for REST API calls. PouchDB authenticates directly with this session token. Both tokens are held in memory only and are never written to `localStorage` or cookies.

The browser replication filter excludes documents the browser has no need for:

```json
{
  "$or": [
    { "type": "file",               "status": "active" },
    { "type": "virtual_directory" },
    { "type": "node" },
    { "type": "agent_status" },
    { "type": "label_assignment" },
    { "type": "label_rule" },
    { "type": "plugin" },
    { "type": "annotation" },
    { "type": "access" },
    { "type": "storage_backend" },
    { "type": "replication_rule" },
    { "type": "replica" },
    { "type": "notification" }
  ]
}
```

`credential` documents are excluded — the browser never needs to see secret key hashes, and the `mosaicfs_browser` role does not have read access to them even if the filter were misconfigured. `utilization_snapshot` documents are excluded because the browser fetches snapshot history on demand via the REST API rather than syncing the full time series into PouchDB.

**Browser replica size.** The browser replicates all active `file` documents, which at 500K files (~500 bytes each) is approximately 250 MB of IndexedDB storage. Modern browsers typically allow 1–2 GB per origin before prompting the user. At the target scale this is within budget, but warrants monitoring. The web UI displays the PouchDB replica size on the Settings page. If the replica approaches 500 MB, the UI displays a warning suggesting the user reduce indexed file counts or await a future version with server-side pagination that eliminates the need for full file document replication in the browser. v1 does not implement client-side purging because PouchDB purge support is limited and could interfere with the replication checkpoint.

**Deleted file tombstone propagation**

Excluding `status: "deleted"` files from agent replication creates a subtle problem: if a file is deleted on node A, agents on other nodes never receive the updated document and their VFS backends continue to list the deleted file indefinitely. The v1 approach sidesteps this by replicating deleted file documents to agents without filtering on `status` — accepting a modestly larger agent replica in exchange for correct deletion propagation. Deleted files are excluded at query time by the `status: "active"` condition applied in rule engine evaluation. The flow 2 filter above reflects this: the `file` selector omits `status: "active"` intentionally, so both active and deleted file documents replicate to agents.

When a file document transitions to `status: "deleted"`, it immediately drops out of any directory listing on the next `readdir` evaluation — the rule engine's step pipeline checks `status: "active"` before evaluating mount steps, so deleted files never appear as virtual directory contents regardless of what the mount sources say.

**Replication document setup**

Each agent sets up two continuous replication jobs on startup by writing documents to its local CouchDB `_replicator` database. If replication documents already exist (from a previous run), the agent leaves them in place — CouchDB resumes replication automatically from the last checkpoint.

Flow 1 (push) replication document:

```json
{
  "_id": "mosaicfs-push",
  "source": "mosaicfs",
  "target": "https://<control_plane_host>:<port>/api/replication/mosaicfs",
  "continuous": true,
  "selector": {
    "$or": [
      { "type": "file",                 "source.node_id": "<this_node_id>" },
      { "type": "node",                 "_id":            "node::<this_node_id>" },
      { "type": "agent_status",         "node_id":        "<this_node_id>" },
      { "type": "utilization_snapshot", "node_id":        "<this_node_id>" },
      { "type": "annotation",           "source.node_id": "<this_node_id>" },
      { "type": "access",               "source.node_id": "<this_node_id>" },
      { "type": "replica",              "source.node_id": "<this_node_id>" },
      { "type": "notification",         "source.node_id": "<this_node_id>" }
    ]
  },
  "create_target": false,
  "_replication_state_reason": null
}
```

Flow 2 (pull) replication document:

```json
{
  "_id": "mosaicfs-pull",
  "source": "https://<control_plane_host>:<port>/api/replication/mosaicfs",
  "target": "mosaicfs",
  "continuous": true,
  "selector": {
    "$or": [
      { "type": "file" },
      { "type": "virtual_directory" },
      { "type": "node" },
      { "type": "credential",       "enabled": true },
      { "type": "label_assignment" },
      { "type": "label_rule" },
      { "type": "plugin" },
      { "type": "annotation" },
      { "type": "access" },
      { "type": "storage_backend" },
      { "type": "replication_rule" },
      { "type": "replica" },
      { "type": "notification" }
    ]
  },
  "create_target": false,
  "_replication_state_reason": null
}
```

The `target` URL for push and the `source` URL for pull point to the Axum replication proxy endpoint, which forwards CouchDB replication protocol requests to the local CouchDB instance after validating the agent's HMAC-signed request headers. The agent reads the control plane URL and its own node ID from `agent.toml`. TLS verification uses the CA certificate stored in the agent's `certs/ca.crt`.

Flow 3 (browser pull) is not configured by the agent. The browser's PouchDB client initiates replication directly against the CouchDB `_changes` feed, authenticating with the short-lived `mosaicfs_browser` session token issued by the Axum login endpoint.

**Replication health monitoring**

The agent monitors replication state by watching the `_replication_state` field on both replication documents. If either enters the `error` state, the agent logs the `_replication_state_reason`, writes a notification document (`notification::<node_id>::replication_error`), and waits for CouchDB's built-in retry mechanism to re-establish the connection. The agent does not delete and recreate replication documents on transient errors — CouchDB handles retry internally with exponential backoff.

---

