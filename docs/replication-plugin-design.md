# Replication Plugin Design

> **Note:** This document was the original design exploration for file replication. Replication has since been split into **core replication** (rule evaluation, replica tracking, Tier 4b failover, restore operations) documented in `architecture.md`, and **storage backend plugins** (thin I/O adapters for S3, B2, etc.) also documented in `architecture.md` under "Storage Backends." This document is retained as supplementary reference for the storage backend plugin design details and example configurations that informed the final architecture.

## Overview

MosaicFS provides selective file replication as a core feature, with the actual I/O to external storage services handled by thin storage backend plugins. The core agent handles rule evaluation, scheduling, bandwidth management, state tracking (via `replica` documents in CouchDB), and failover. Storage backend plugins handle only uploading, downloading, deleting, and listing files on a specific storage service.

This document covers the original design exploration, including details on storage backend plugin implementation, example configurations, and operational considerations that supplement the architecture document.

## Design Principles

- **Replication is core, storage backends are plugins.** Rule evaluation, replica state tracking, scheduling, and failover are handled by the agent's replication subsystem. Storage backend plugins are thin I/O adapters.
- **Reuse the step pipeline vocabulary.** Replication rules use the same step operations as virtual directory mount sources. The agent evaluates them using the same engine and caches.
- **Replica state in CouchDB.** `replica` documents are first-class CouchDB documents, replicated to all nodes. The VFS reads them directly for Tier 4b failover — no plugin indirection.
- **Socket plugins for storage backends.** Long-running process maintains authenticated sessions and connection pools, amortizing OAuth/TLS costs across invocations.

## Plugin Configuration

The replication plugin is configured via the standard plugin document, with targets and rules defined in the `config` field:

```json
{
  "_id": "plugin::node-laptop::replication",
  "type": "plugin",
  "node_id": "node-laptop",
  "plugin_name": "replication",
  "plugin_type": "socket",
  "enabled": true,
  "name": "File Replication",
  "subscribed_events": ["file.added", "file.modified", "file.deleted", "access.updated"],
  "mime_globs": ["*/*"],
  "workers": 1,
  "timeout_s": 300,
  "max_attempts": 3,
  "query_endpoints": [
    {
      "name": "replication-status",
      "capability": "dashboard_widget",
      "description": "Replication status and statistics"
    }
  ],
  "settings_schema": {
    "properties": {
      "flush_interval_s": {
        "type": "integer",
        "title": "Status flush interval (seconds)",
        "default": 60
      }
    }
  },
  "settings": {
    "flush_interval_s": 60
  },
  "config": {
    "targets": [
      {
        "target_name": "offsite-backup",
        "backend": "s3",
        "bucket": "my-backups",
        "prefix": "mosaicfs/",
        "region": "us-east-1",
        "credentials_ref": "backup-s3-key",
        "schedule": "02:00-06:00",
        "bandwidth_limit_mbps": 50,
        "retention": { "keep_deleted_days": 30 }
      },
      {
        "target_name": "local-archive",
        "backend": "directory",
        "path": "/mnt/external/archive",
        "retention": { "keep_deleted_days": 0 }
      }
    ],
    "rules": [...]
  },
  "dispatch_rules": [...]
}
```

### Targets

Each target defines a destination for replicated files.

| Field | Type | Description |
|---|---|---|
| `target_name` | string | Unique identifier for this target within the plugin. Referenced by rules. |
| `backend` | string | `"s3"`, `"b2"`, `"directory"`, or `"agent"`. Determines which fields are required. |
| `schedule` | string? | Optional time window in `HH:MM-HH:MM` format (24h, local time). When set, the plugin queues matched events but only transfers during the window. Omit for continuous replication. |
| `bandwidth_limit_mbps` | int? | Optional upload bandwidth cap in megabits per second. Enforced via token bucket rate limiter. |
| `retention.keep_deleted_days` | int | How many days to retain a file on the target after it is deleted from MosaicFS. `0` means delete from the target immediately on `file.deleted`. |

**Target type-specific fields:**

| Target Type | Fields |
|---|---|
| `s3` | `bucket`, `prefix`, `region`, `credentials_ref` (references a key in the plugin's `settings`), `storage_class` (optional, e.g. `"GLACIER"`) |
| `b2` | `bucket`, `prefix`, `credentials_ref` |
| `directory` | `path` (absolute path on the local filesystem; must be excluded from agent crawl paths) |
| `agent` | `node_id` (destination agent), `path_prefix` (where to write on the destination; must be excluded from that agent's crawl paths) |

### Rules

Each rule defines which files should be replicated to which target. Rules use the same step pipeline operations as virtual directory mount sources.

```json
{
  "rules": [
    {
      "name": "Archive cold files to S3",
      "target": "offsite-backup",
      "source": { "node_id": "*" },
      "steps": [
        { "op": "label", "labels": ["important"] },
        { "op": "access_age", "min_days": 90, "missing": "include", "on_match": "include" }
      ],
      "default_result": "exclude"
    },
    {
      "name": "Back up all work documents",
      "target": "offsite-backup",
      "source": { "node_id": "node-laptop", "path_prefix": "/home/mark/projects/" },
      "steps": [
        { "op": "glob", "pattern": "**/.git/**", "invert": true, "on_match": "continue" },
        { "op": "size", "max_bytes": 104857600 }
      ],
      "default_result": "include"
    },
    {
      "name": "Archive large media locally",
      "target": "local-archive",
      "source": { "node_id": "*" },
      "steps": [
        { "op": "mime", "types": ["video/*", "audio/*"] },
        { "op": "size", "min_bytes": 104857600 },
        { "op": "access_age", "min_days": 180, "missing": "include", "on_match": "include" }
      ],
      "default_result": "exclude"
    }
  ]
}
```

**Rule fields:**

| Field | Type | Description |
|---|---|---|
| `name` | string | Human-readable name. Included in the `matched_rules` array in the event envelope. |
| `target` | string | The `target_name` to replicate to. |
| `source.node_id` | string | Filter to files from a specific node, or `"*"` for all nodes. |
| `source.path_prefix` | string? | Optional path prefix filter. Only files whose `export_path` starts with this prefix are considered. |
| `steps` | array | Step pipeline operations — same syntax and semantics as virtual directory mount steps. All nine ops are supported: `glob`, `regex`, `age`, `size`, `mime`, `node`, `label`, `access_age`, `annotation`. |
| `default_result` | string | `"include"` or `"exclude"`. Applied when no step short-circuits. |

A file may match multiple rules targeting different (or the same) targets. Each match produces an independent replication action.

## Agent-Assisted Rule Evaluation

### The Problem

The replication plugin needs to evaluate rules that reference the materialized label cache and materialized access cache — both are in-memory data structures inside the agent process. A plugin running as a separate process cannot access them directly. Re-implementing the caches in the plugin would be wasteful and error-prone.

### The Solution: `dispatch_rules`

The plugin document schema gains a new optional field, `dispatch_rules`, which instructs the agent to evaluate rules through the existing step pipeline engine *before* dispatching events to the plugin.

```json
{
  "dispatch_rules": [
    {
      "name": "Archive cold files to S3",
      "source": { "node_id": "*" },
      "steps": [
        { "op": "label", "labels": ["important"] },
        { "op": "access_age", "min_days": 90, "missing": "include", "on_match": "include" }
      ],
      "default_result": "exclude"
    }
  ]
}
```

The `dispatch_rules` array is derived directly from the plugin's `config.rules` — in practice the replication plugin writes both fields with the same content (minus the `target` field, which is plugin-specific and not meaningful to the agent). The agent does not interpret `config.rules`; it only evaluates `dispatch_rules`.

**Dispatch behavior:**

1. A file event arrives (e.g. `file.modified`).
2. The agent checks if the file matches `subscribed_events` and `mime_globs` (existing behavior).
3. If `dispatch_rules` is present, the agent evaluates the file against each rule using the step pipeline engine. This has access to the label cache, access cache, and annotation index — same as readdir evaluation.
4. If at least one rule matches, the event is dispatched to the plugin with a `matched_rules` field listing the names of all matching rules.
5. If no rules match, the event is suppressed — the plugin never sees it.

**Event envelope with matched rules:**

```json
{
  "event": "file.modified",
  "sequence": 4207,
  "timestamp": "2026-02-19T03:00:00Z",
  "node_id": "node-laptop",
  "payload": {
    "file_id": "file::node-laptop::a3f9...",
    "export_path": "/home/mark/projects/report.pdf",
    "name": "report.pdf",
    "size": 204800,
    "mime_type": "application/pdf",
    "mtime": "2026-02-18T14:30:00Z"
  },
  "matched_rules": ["Archive cold files to S3", "Back up all work documents"],
  "config": { ... }
}
```

The `file.deleted` event bypasses `dispatch_rules` evaluation — the plugin always receives deletion events for files it has previously replicated. The plugin determines which targets are affected by checking its local SQLite state.

The `access.updated` event uses a hybrid dispatch: the agent evaluates `dispatch_rules` and also checks whether the file has an existing annotation from this plugin. If either condition is true, the event is dispatched. This ensures the plugin hears about access changes to files it has replicated, even when those changes cause the file to fall out of the rules — which is precisely the signal the plugin needs to trigger un-replication.

### Generality

`dispatch_rules` is not replication-specific. Any plugin can use it to receive only events for files matching specific criteria. Examples:

- A thumbnail generator that only wants images under 10 MB
- A compliance checker that only wants files with a "confidential" label
- A transcription plugin that only wants audio files older than 1 day (to avoid processing in-progress recordings)

The existing `mime_globs` filter is effectively a single dispatch rule with one `mime` step. Both coexist — `mime_globs` is evaluated first (cheap string match), then `dispatch_rules` (step pipeline evaluation). Over time `mime_globs` could be considered syntactic sugar for a dispatch rule, but there is no need to deprecate it.

## Plugin State

### Local SQLite Database

The replication plugin maintains a SQLite database in its working directory (`replication.db`). This is not replicated — it is local to the plugin process. CouchDB annotations provide cross-system visibility; SQLite provides the plugin's internal bookkeeping.

```sql
CREATE TABLE replication_state (
    file_id         TEXT NOT NULL,
    target_name     TEXT NOT NULL,
    replicated_at   TEXT NOT NULL,       -- ISO 8601
    source_mtime    TEXT NOT NULL,       -- mtime at time of replication
    source_size     INTEGER NOT NULL,    -- size at time of replication
    remote_key      TEXT NOT NULL,       -- S3 key, B2 file ID, local path, etc.
    checksum        TEXT,                -- SHA-256 of replicated content
    PRIMARY KEY (file_id, target_name)
);

CREATE TABLE deletion_log (
    file_id         TEXT NOT NULL,
    target_name     TEXT NOT NULL,
    deleted_at      TEXT NOT NULL,       -- when file.deleted was received
    retain_until    TEXT,                -- NULL = delete immediately; otherwise ISO 8601
    remote_key      TEXT NOT NULL,       -- key on target, needed for cleanup
    purged          INTEGER DEFAULT 0,   -- 1 = removed from target storage
    PRIMARY KEY (file_id, target_name)
);

CREATE INDEX idx_deletion_retain ON deletion_log (purged, retain_until);
```

**Why SQLite, not CouchDB:**

- The manifest can be large (one row per replicated file per target — up to 500K rows per target at scale). Storing this as CouchDB documents would create substantial replication volume for purely internal state.
- The plugin needs transactional updates: "upload succeeded, update manifest row" must be atomic. SQLite provides this naturally.
- The manifest is recoverable — a lost SQLite database can be rebuilt by scanning the target and comparing against the file index. Slow, but possible. An annotation-only approach would make this harder.

### CouchDB Annotations

The plugin writes one annotation per replicated file. These are lightweight and serve two purposes: making replication status visible to the rest of MosaicFS (search, virtual directory rules, web UI), and providing a coarse recovery signal if the local SQLite database is lost.

```json
{
  "_id": "annotation::node-laptop::sha256(/home/mark/projects/report.pdf)::replication",
  "type": "annotation",
  "node_id": "node-laptop",
  "export_path": "/home/mark/projects/report.pdf",
  "plugin_name": "replication",
  "status": "ok",
  "annotated_at": "2026-02-19T03:15:00Z",
  "data": {
    "targets": {
      "offsite-backup": {
        "replicated_at": "2026-02-19T03:15:00Z",
        "status": "current"
      },
      "local-archive": {
        "replicated_at": "2026-02-18T02:00:00Z",
        "status": "stale"
      }
    }
  }
}
```

**Annotation `status` values per target:**

| Status | Meaning |
|---|---|
| `"current"` | The file has been replicated and the target copy matches the source mtime/size. |
| `"stale"` | The file has been modified since it was last replicated to this target. Set when a `file.modified` event is processed before the new version is uploaded. |
| `"pending"` | The file matched a rule but has not been replicated yet (queued or waiting for schedule window). |
| `"frozen"` | The file was previously replicated but no longer matches any rule for this target. The existing copy on the target is preserved but not maintained — future source modifications will not be synced. Only set when `remove_unmatched` is `false`. |
| `"failed"` | Replication failed after `max_attempts`. The plugin will retry on the next `file.modified` or manual sync trigger. |

Annotations are updated in batches (not per-file) to limit CouchDB write volume. The plugin accumulates annotation changes in memory and flushes them every `flush_interval_s` seconds (default 60).

**Using replication status in virtual directories:**

The annotation system enables filtering on replication status in mount step pipelines:

```json
{ "op": "annotation", "plugin": "replication", "key": "targets.offsite-backup.status", "value": "current" }
```

Example use cases:
- A "Not backed up" virtual directory: `{ "op": "annotation", "plugin": "replication", "key": "targets.offsite-backup.status", "value": "current", "invert": true, "on_match": "include" }`
- A "Pending replication" view: `{ "op": "annotation", "plugin": "replication", "key": "targets.offsite-backup.status", "value": "pending", "on_match": "include" }`

**Using replication status in search:**

```
GET /api/search?q=report&annotation[replication/targets.offsite-backup.status]=current
```

## Transfer and Upload

### Fetching File Bytes

The plugin fetches file content from the local agent's transfer endpoint:

```
GET /api/agent/transfer/{file_id}
```

This uses the existing tiered access strategy — the agent resolves the cheapest path to the file bytes (local, network mount, cache, remote fetch, plugin materialize) transparently. The replication plugin does not need to know where the file physically lives.

For the `agent` target type (replicating to another MosaicFS agent), the plugin uploads via the destination agent's transfer endpoint. This requires a new endpoint on the receiving agent — see "Changes to Core" below.

### Upload to External Storage

The plugin manages uploads internally using the appropriate SDK or protocol:

| Target Type | Upload Method |
|---|---|
| `s3` | AWS SDK multipart upload. Connection pooling across batch. |
| `b2` | B2 native API. Large file upload for files > 100 MB. |
| `directory` | Atomic write: write to temp file, `fsync`, rename. |
| `agent` | `POST /api/agent/replicate/{file_id}` on the destination agent. |

### Remote Key Scheme

Files are stored on the target using a deterministic key derived from their MosaicFS identity:

```
{prefix}/{file_uuid_8}/{filename}

Example:
mosaicfs/a3f92b1c/report.pdf
```

The `file_uuid_8` is the first 8 characters of the file's UUID. This provides shard-like distribution on S3 while keeping filenames human-readable. The full file metadata is recoverable from the file document via `file_id`.

### Bandwidth and Scheduling

- **Schedule windows**: The plugin maintains an internal event queue. Events that arrive outside the schedule window are queued in memory (backed by SQLite for durability). When the window opens, the queue is drained in FIFO order.
- **Bandwidth limiting**: A token bucket rate limiter wraps the upload I/O. The bucket refills at `bandwidth_limit_mbps` and is shared across all concurrent uploads to the same target.
- **Batching**: The plugin processes events in batches of up to 100. For S3/B2, this enables connection reuse and amortizes authentication overhead. For directory targets, this enables a single `fsync` of the parent directory after a batch of writes.
- **Concurrency**: Configurable `workers` field on the plugin document controls how many files are uploaded in parallel per target. Default 2.

## Deletion and Retention

When the plugin receives a `file.deleted` event:

1. Look up the file in `replication_state` for each target.
2. If the file is not tracked for any target, ignore the event.
3. For each target where the file is tracked:
   a. If `retention.keep_deleted_days == 0`: delete from the target immediately, remove from `replication_state`, remove the target entry from the annotation.
   b. If `retention.keep_deleted_days > 0`: move the row from `replication_state` to `deletion_log` with `retain_until` set to `deleted_at + keep_deleted_days`. Remove the target entry from the annotation.
4. A background sweep runs hourly, scanning `deletion_log` for rows where `retain_until < now AND purged = 0`. For each, delete from the target and set `purged = 1`.

**Restore from retention**: During the retention window, a deleted file's copy still exists on the target. Restoring it is a manual operation in v1 — the user downloads from the target directly. A future version could add a restore command that re-creates the file document and triggers the owning agent to re-index.

## Rule Re-evaluation

File events alone are insufficient for rules that reference time-varying predicates like `access_age`. There are two distinct re-evaluation problems: files that *become* eligible over time, and files that *stop* being eligible.

### Becoming eligible: periodic full scan

A file that was recently accessed when the rule was first evaluated may become "cold" 90 days later without generating any file event. No event fires when a threshold is crossed — the system only learns about it by re-checking.

The plugin triggers a **periodic full scan** (configurable, default daily) via the existing plugin full sync mechanism (`POST /api/nodes/{node_id}/plugins/replication/sync`), which enqueues `file.added` events for all active files. The agent evaluates `dispatch_rules` on each, so the plugin only receives files that currently match. The plugin compares each event against its SQLite manifest and uploads files that are new or stale.

The periodic scan also detects:
- Files that are now stale on the target (source mtime/size changed but no `file.modified` event was received — e.g. due to a missed watcher event)
- Files that newly match a rule due to changed labels, annotations, or access age

### Losing eligibility: access events and un-replication

The opposite direction is also important. Consider a rule: "replicate files not accessed in 30 days." A replicated file that gets accessed should eventually be un-replicated (removed from the target) since it no longer matches the rule. There are two mechanisms for detecting this:

**1. `access.updated` events.** The plugin subscribes to `access.updated`, a new event type emitted by the agent when flushing a debounced access document update. When the plugin receives an `access.updated` event for a file it has replicated, it re-evaluates the file against `dispatch_rules`. If the file no longer matches any rule for a given target, the plugin marks it for un-replication on that target.

The `access.updated` event envelope:

```json
{
  "event": "access.updated",
  "sequence": 5012,
  "timestamp": "2026-02-19T14:05:00Z",
  "node_id": "node-laptop",
  "payload": {
    "file_id": "file::node-laptop::a3f9...",
    "last_access": "2026-02-19T14:00:00Z",
    "access_count": 43
  },
  "matched_rules": [],
  "config": { ... }
}
```

Note that `matched_rules` may be empty — the agent evaluates `dispatch_rules` and the file may no longer match. The plugin receives the event anyway (because it has the file in its manifest) and uses the empty `matched_rules` as a signal to un-replicate. This requires a refinement to the dispatch logic: for `access.updated` events, the agent dispatches to the plugin if either (a) dispatch rules match, or (b) the file has a replication annotation from this plugin. This ensures the plugin always hears about access changes to files it has replicated, even when those changes cause the file to fall out of the rules.

**2. Periodic full scan (catch-all).** The daily scan also detects files that no longer match any rule. During the scan, the plugin compares its manifest against the set of files that matched dispatch rules. Files present in the manifest but absent from the scan results are candidates for un-replication.

### Un-replication behavior

When a file stops matching all rules for a target, the plugin's behavior depends on the target's `remove_unmatched` setting:

| `remove_unmatched` | Behavior |
|---|---|
| `false` (default) | The file remains on the target indefinitely. The annotation is updated to `"status": "frozen"` — replicated but no longer actively maintained. Future modifications to the source file will not be synced to the target. |
| `true` | The file is moved to the `deletion_log` with `retain_until` set per the target's `retention.keep_deleted_days`. After the retention window expires, it is purged from the target. The annotation target entry is removed. |

The `"frozen"` status is distinct from `"stale"` — stale means the source has changed and the target copy is out of date; frozen means the file is deliberately excluded from future replication but the existing copy is preserved.

## Dashboard Widget

The plugin advertises a `dashboard_widget` query endpoint. When queried, it reads from its local SQLite database:

```json
{
  "plugin_name": "replication",
  "capability": "dashboard_widget",
  "status": "healthy",
  "data": {
    "Files replicated": "12,847",
    "Pending": "23",
    "Failed": "0",
    "Last sync": "3 minutes ago",
    "offsite-backup": "48.2 GB (12,302 files)",
    "local-archive": "124.7 GB (545 files)",
    "Next scheduled window": "02:00 (in 4h 12m)"
  }
}
```

## Health Checks and Notifications

The plugin uses the socket plugin health check mechanism to surface issues:

```json
{
  "status": "degraded",
  "notifications": [
    {
      "condition_key": "target_unreachable:offsite-backup",
      "severity": "error",
      "title": "Replication target unreachable",
      "message": "Cannot connect to S3 bucket 'my-backups' in us-east-1. Last successful upload 2 hours ago. 47 files pending.",
      "actions": [
        { "label": "View pending files", "api": "GET /api/nodes/node-laptop/plugins/replication/jobs?status=pending" }
      ]
    }
  ],
  "resolve_notifications": []
}
```

**Notification conditions:**

| Condition Key | Severity | Triggers |
|---|---|---|
| `target_unreachable:{target_name}` | error | Connection failure to target after 3 retries. Auto-resolves on success. |
| `replication_backlog` | warning | Pending queue exceeds 1,000 files. Auto-resolves when queue drains below 100. |
| `retention_purge_failed:{target_name}` | warning | Failed to delete expired files from target. |
| `manifest_rebuild_needed` | warning | Local SQLite database was lost or corrupted. Plugin is operating in degraded mode until a full scan completes. |

## File Recovery

The primary purpose of replication is to ensure files can be recovered when the source node is unavailable. There are two recovery scenarios with different mechanics.

### Temporary failover (source node offline)

When the source node is offline, the VFS Tier 4 remote fetch fails with a connection error and returns EIO to the caller. But the replication plugin may have a copy of the file on an external target or another agent. The system should be able to serve that copy transparently.

This requires a **new access tier — Tier 4b** — in the VFS tiered access strategy, evaluated when Tier 4 fails because the owning node is offline:

```
Tier 4 fails (owning node offline or unreachable)
  → check file's replication annotation for available replicas
  → for each target with status "current" or "frozen":
      → attempt to fetch from target
      → on success: cache locally, serve from cache
  → if no replicas available or all fetches fail: return EIO
```

**Target fetch resolution by type:**

| Target Type | Fetch Method |
|---|---|
| `agent` | `GET /api/agent/transfer/{file_id}` on the replica agent. The replica agent serves the file from its replication storage path. This reuses the existing transfer mechanism — the replica agent just needs to have the replication storage directory included in its transfer-servable paths (not its crawl paths). |
| `s3` / `b2` | The VFS layer cannot fetch from S3 directly — it has no credentials. Instead, the VFS delegates to the replication plugin on the local node via a `materialize`-like request. The plugin fetches from the target using its configured credentials, writes to a staging path, and the VFS caches the result. |
| `directory` | If the directory is on a locally accessible filesystem (e.g. a NAS mount), the VFS opens the file directly using the remote key from the annotation. If not locally accessible, EIO. |

**What the VFS needs to implement Tier 4b:**

- Read the file's replication annotation (already available in the local CouchDB replica).
- For `agent` targets: attempt transfer from the replica node, same as Tier 4 but targeting a different node.
- For `s3`/`b2` targets: invoke the local replication plugin's materialize capability. This requires the replication plugin to handle a new event type: `materialize_from_replica`.

**`materialize_from_replica` event:**

```json
{
  "event": "materialize_from_replica",
  "payload": {
    "file_id": "file::node-laptop::a3f9...",
    "target_name": "offsite-backup",
    "staging_path": "/var/lib/mosaicfs/cache/tmp/replica-a3f9..."
  },
  "config": { ... }
}
```

The plugin fetches the file from the named target using its credentials and remote key (looked up from the SQLite manifest or, if the manifest is unavailable, derived from the file_id using the remote key scheme). It writes bytes to `staging_path` and responds with `{ "size": N }`. The agent moves the staging file into the VFS cache, same as Tier 5 materialize.

**Tier 4b is best-effort.** It only works when:
- The file has a replication annotation with at least one `"current"` or `"frozen"` target
- For `agent` targets: the replica node is online
- For `s3`/`b2` targets: the replication plugin is running on the local node and has credentials for the target
- For `directory` targets: the directory is locally accessible

When none of these conditions are met, the VFS returns EIO as before. The user sees the same failure they would see without replication — replication adds opportunistic resilience, not guaranteed availability.

### Full restore (source node permanently lost)

When a source node is permanently destroyed, its file documents remain in CouchDB with `source.node_id` pointing to a node that will never come back online. The files exist on replication targets but are not accessible through the normal VFS because Tier 4 always fails and the node will never recover.

Full restore is an explicit operation — not automatic failover. It re-creates file documents that point to a new source location, making the files permanently accessible again.

**Restore to an existing agent (from `agent` target):**

The simplest case. The replica files already exist on a real filesystem on the destination agent. The restore operation:

1. User initiates restore via CLI or web UI: `POST /api/plugins/replication/restore` with `{ "target_name": "nas-mirror", "source_node_id": "node-laptop", "destination_node_id": "node-nas" }`.
2. The control plane (or the plugin on the destination agent) scans the replication storage directory on the destination node.
3. For each file found, the existing file document's `source.node_id` is updated to `"node-nas"` and `source.export_path` set to the replica's location in the replication storage directory (migration-style ownership transfer). Because file identity is location-independent (`file::{uuid}`), all label assignments, annotations, and access documents carry over automatically.
4. The destination agent's crawler picks up the updated documents on the next sync.

**Restore from external storage (`s3` / `b2`):**

More involved — the files need to be downloaded from the target and written to a local filesystem.

1. User initiates restore: `POST /api/plugins/replication/restore` with `{ "target_name": "offsite-backup", "source_node_id": "node-laptop", "destination_node_id": "node-nas", "destination_path": "/mnt/raid/restored/" }`.
2. The replication plugin on the destination node lists objects on the target, downloads each file to `destination_path` (preserving the original directory structure derived from the export path hash → full path mapping in the manifest or annotation data).
3. For each downloaded file, the existing file document's `source.node_id` is updated to `"node-nas"` and `source.export_path` set under `destination_path` (migration-style ownership transfer). All label assignments, annotations, and access documents carry over automatically.
4. The destination path must be included in the agent's crawl paths so the agent can maintain the documents going forward.

**Restore preserves file identity.** Because file `_id` is `file::{uuid}` (location-independent), restoring a file updates its ownership fields but keeps its identity. All label assignments, annotations, replicas, and access documents continue to reference the same file without changes. Virtual directory rules that use `node_id: "*"` will include restored files automatically; rules that reference the original `node_id` will need updating.

**Partial restore.** The restore endpoint supports optional filters to restore a subset of files: `{ "path_prefix": "/home/mark/projects/", "mime_type": "application/pdf" }`. This allows restoring specific directories rather than the entire replication set.

### REST API for restore

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/plugins/replication/restore` | Initiate a restore operation. Request body specifies `target_name`, `source_node_id` (the lost node), `destination_node_id`, and optional filters. Returns a job ID for tracking progress. |
| `GET` | `/api/plugins/replication/restore/{job_id}` | Check restore progress: files scanned, downloaded, created, errors. |
| `POST` | `/api/plugins/replication/restore/{job_id}/cancel` | Cancel an in-progress restore. Files already restored remain; no rollback. |
| `GET` | `/api/plugins/replication/restore/history` | List past restore operations with summaries. |

## Plugin State Recovery

### Lost SQLite Database

If the plugin's `replication.db` is lost (disk failure, accidental deletion):

1. The plugin detects the missing database on startup and creates a fresh one.
2. It enters "rebuild mode" and issues a `manifest_rebuild_needed` notification.
3. On the next periodic full scan, it compares the target's contents against the current file index to reconstruct `replication_state`.
4. For S3/B2: list objects under the configured prefix, parse the key scheme to recover `file_id` mappings.
5. For directory targets: walk the directory tree.
6. Files present on the target but absent from MosaicFS are flagged for retention review.
7. After rebuild completes, the notification auto-resolves.

During rebuild mode, new replication events are processed normally — they just won't benefit from incremental sync (the plugin may re-upload files that already exist on the target, which is wasteful but correct).

### Lost Annotations

Annotations can be rebuilt from the SQLite manifest. The plugin does this automatically as part of its periodic annotation flush.

## Changes to Core

| Area | Change | Scope |
|---|---|---|
| Plugin document schema | Add optional `dispatch_rules` field (array of rule objects with `name`, `source`, `steps`, `default_result`) | Small — additive, no existing fields change |
| Plugin event envelope | Add optional `matched_rules` field (array of rule name strings) | Small — additive |
| Plugin event types | Add `access.updated` event, emitted by the agent when flushing a debounced access document update | Small — new event type, follows existing event patterns |
| Agent plugin dispatch | When `dispatch_rules` is present, evaluate each file event against the rules using the step pipeline engine before dispatching. Suppress events that match no rules. For `access.updated` events, also dispatch if the file has an existing annotation from this plugin (ensures the plugin hears about access changes to files it has replicated). | Medium — new code path in the dispatch hot path, but reuses the existing rule evaluation engine and caches |
| VFS tiered access | Add Tier 4b: when Tier 4 fails (owning node offline), check replication annotations for available replicas. For `agent` targets, attempt transfer from replica node. For `s3`/`b2` targets, invoke the replication plugin's `materialize_from_replica` capability. | Medium — new tier in the access chain, reads annotations and conditionally invokes plugin |
| Plugin materialize | Add `materialize_from_replica` event type for plugins that can fetch files from external replication targets on behalf of the VFS layer | Small — follows existing `materialize` event pattern |
| Agent config | Add `excluded_paths` array to `agent.toml` to prevent the crawler from indexing replication storage directories. Add `transfer_serve_paths` to allow the transfer endpoint to serve files from replication storage without crawling them. | Small |
| Agent transfer (for `agent` target type) | Add `POST /api/agent/replicate` endpoint that accepts file bytes from a replication plugin on a remote agent and writes them to a configured replication storage path | Small — only needed for agent-to-agent replication target type |
| REST API | Add `/api/plugins/replication/restore` endpoints for initiating and monitoring restore operations | Small — plugin-scoped REST routes |

The `dispatch_rules` enhancement and the Tier 4b access path are the two changes with meaningful complexity. Both are general-purpose: `dispatch_rules` benefits any plugin that wants filtered events; Tier 4b enables any plugin that maintains file replicas to serve as a fallback source.

## Example Configurations

### Simple backup of a project directory

```json
{
  "targets": [
    { "target_name": "backup", "backend": "s3", "bucket": "my-backups", "prefix": "projects/", "region": "us-east-1", "credentials_ref": "aws-key", "retention": { "keep_deleted_days": 90 } }
  ],
  "rules": [
    {
      "name": "All project files",
      "target": "backup",
      "source": { "node_id": "node-laptop", "path_prefix": "/home/mark/projects/" },
      "steps": [
        { "op": "glob", "pattern": "**/{.git,node_modules,target,__pycache__}/**", "invert": true, "on_match": "continue" },
        { "op": "size", "max_bytes": 524288000 }
      ],
      "default_result": "include"
    }
  ]
}
```

### Archive cold media to cheap storage

```json
{
  "targets": [
    { "target_name": "glacier", "backend": "s3", "bucket": "media-archive", "prefix": "cold/", "region": "us-east-1", "credentials_ref": "aws-key", "storage_class": "GLACIER", "schedule": "02:00-06:00", "bandwidth_limit_mbps": 20, "retention": { "keep_deleted_days": 365 } }
  ],
  "rules": [
    {
      "name": "Cold large media",
      "target": "glacier",
      "source": { "node_id": "*" },
      "steps": [
        { "op": "mime", "types": ["video/*", "audio/*", "image/*"] },
        { "op": "size", "min_bytes": 52428800 },
        { "op": "access_age", "min_days": 180, "missing": "include", "on_match": "include" }
      ],
      "default_result": "exclude"
    }
  ]
}
```

### Cross-node redundancy for important files

```json
{
  "targets": [
    { "target_name": "nas-mirror", "backend": "agent", "node_id": "node-nas", "path_prefix": "/mnt/raid/mosaicfs-replicas/", "retention": { "keep_deleted_days": 30 } }
  ],
  "rules": [
    {
      "name": "Important labeled files",
      "target": "nas-mirror",
      "source": { "node_id": "node-laptop" },
      "steps": [
        { "op": "label", "labels": ["important"], "on_match": "include" }
      ],
      "default_result": "exclude"
    }
  ]
}
```
