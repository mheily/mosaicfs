<\!-- MosaicFS Architecture · ../architecture.md -->

## CouchDB Document Schemas

All data in MosaicFS is stored as JSON documents in CouchDB. Each document type has a `type` field that identifies its role in the system.

### File Document

Represents a single file on a physical node or cloud service. Created and updated by the agent crawler and watcher. Carries only intrinsic properties of the file — where it physically lives and what it looks like. Virtual locations are computed on demand by evaluating virtual directory mount sources; they are not stored on the file document.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"file::{uuid}"`. The UUID is generated at document creation time. Unique across the system. The file's location (which node owns it, what path it lives at) is stored in `source` fields, not encoded in the `_id`. This makes file identity location-independent — a file can be migrated between nodes without changing its `_id` or any documents that reference it. |
| `type` | string | Always `"file"`. |
| `inode` | uint64 | Random 64-bit integer assigned at creation time. Stable for the lifetime of the file. Used as the inode number by the FUSE backend, and as the equivalent stable identity token by other VFS backends. A file appearing in multiple virtual directories presents the same inode in each — the OS treats these as hard links. Collision probability at 500K files is ~7×10⁻⁹ (birthday paradox with 2⁶⁴ space); no collision detection is implemented. If a collision did occur, the VFS layer would return the first matching document — an acceptable degradation at this probability. |
| `name` | string | Filename component only (no directory path). Stored as-is from the filesystem — no Unicode normalization is applied, preserving round-trip fidelity on case-sensitive filesystems. Names containing null bytes, forward slashes, or control characters (U+0000–U+001F) are rejected by the crawler and not indexed. The VFS layer does not perform additional sanitization — names valid in CouchDB are presented to the OS as-is, and the OS rejects any that violate its own rules (e.g. `:` on Windows). |
| `source.node_id` | string | ID of the node that owns this file. |
| `source.export_path` | string | The path this node uses to identify this file. For physical agents: absolute filesystem path. For source-mode storage backends: path within the cloud service namespace. For federated peers: virtual path on the peer instance. |
| `source.export_parent` | string | Parent directory component of `export_path`. Used by the rule engine when evaluating `prefix_replace` mount sources — enables efficient lookup of all files under a given real directory. |
| `size` | uint64 | File size in bytes. |
| `mtime` | string | ISO 8601 last-modified timestamp. |
| `mime_type` | string? | MIME type if determinable. |
| `status` | string | `"active"` or `"deleted"`. Soft deletes preserve history. |
| `deleted_at` | string? | ISO 8601 timestamp if `status` is `"deleted"`. |
| `migrated_from` | object? | Present if this file was migrated from another node. Contains `{ node_id, export_path, migrated_at }` — the previous owner's identity and the ISO 8601 timestamp of the migration. Useful for audit trail and debugging. |

### Virtual Directory Document

Represents a directory in the virtual filesystem namespace. Virtual directories are the primary configuration surface in MosaicFS — each directory carries a `mounts` array that defines what files and subdirectories appear inside it. Directories are created and deleted explicitly by the user; they are never created or removed automatically.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"dir::sha256({virtual_path})"`. Deterministic — enables idempotent creation. |
| `type` | string | Always `"virtual_directory"`. |
| `inode` | uint64 | Random 64-bit integer. Inode 1 is reserved for the root directory. |
| `virtual_path` | string | Full path in the virtual namespace, e.g. `"/documents/work"`. |
| `name` | string | Directory name component only. |
| `parent_path` | string? | Parent virtual path. Null for the root directory. |
| `system` | bool? | True for the root and other well-known synthetic entries. Prevents accidental deletion. |
| `created_at` | string | ISO 8601 creation timestamp. |
| `enforce_steps_on_children` | bool | Default `false`. When `true`, this directory's own step pipeline (if any) is prepended to the evaluation of every mount in every descendant directory. Children can add further steps but cannot override or bypass ancestor steps. |
| `mounts` | array | Ordered list of mount sources. Each entry defines a source of files or subdirectories to mount into this directory. See mount entry fields below. |
| `mounts[].mount_id` | string | Short random identifier for this mount entry. Used by the API to target a specific mount for update or deletion. |
| `mounts[].source` | object | Source descriptor. Either `{node_id, export_path}` for a local or cloud node, or `{federated_import_id}` for a federated peer. `node_id` may be `"*"` to match all nodes. |
| `mounts[].strategy` | string | `"prefix_replace"` or `"flatten"`. `prefix_replace` strips the source prefix and mounts the remaining path hierarchy as a subtree. `flatten` places all matching files directly in this directory, discarding subdirectory structure. |
| `mounts[].source_prefix` | string? | Path prefix to strip from `export_path`. Required for `prefix_replace`. |
| `mounts[].steps` | array | Ordered filter steps. Same schema as before — `op`, `invert`, `on_match`, and op-specific parameters. Evaluated after any inherited ancestor steps. |
| `mounts[].default_result` | string | Default `"include"`. Result if all steps complete without a short-circuit. |
| `mounts[].conflict_policy` | string | `"last_write_wins"` or `"suffix_node_id"`. Applied when two sources produce a file at the same name within this directory. |

**Inheritance.** When `enforce_steps_on_children` is `true` on an ancestor directory, its steps are prepended to every mount evaluation in all descendant directories — from outermost ancestor to nearest parent, in that order. A child directory's own mount steps are appended last. This means ancestor steps evaluate first and cannot be bypassed: a child can narrow a parent's results further but cannot surface files the parent has excluded.

**Multiple appearances.** A file may appear in multiple virtual directories simultaneously. The rule engine evaluates each directory's mounts independently — there is no global deduplication. A `proposal.docx` modified yesterday might satisfy both a "Recent documents" directory (matched by an age step) and a "Work documents" directory (matched by a path source). Both are valid virtual locations for the same file. The file document's `inode` is the same in both listings, so the OS treats the two directory entries as hard links to the same file.

### Node Document

Represents a device participating in the MosaicFS network. The `storage` array describes the filesystem and disk topology visible to the agent. Point-in-time utilization figures are recorded in separate `utilization_snapshot` documents rather than here, keeping the node document stable and the snapshot history queryable.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"node::{node_id}"`. |
| `type` | string | Always `"node"`. |
| `friendly_name` | string | Human-readable display name, e.g. `"MacBook Pro"`. |
| `platform` | string | `"linux"`, `"darwin"`, or `"windows"`. |
| `status` | string | `"online"`, `"offline"`, or `"degraded"`. |
| `last_heartbeat` | string | ISO 8601 timestamp of last heartbeat. |
| `vfs_capable` | bool | Whether this node can run a virtual filesystem backend. True for physical nodes with a supported OS. Used by the web UI to indicate which nodes support the filesystem mount. |
| `vfs_backend` | string? | The VFS backend active on this node, if any: `"fuse"`, `"file_provider"`, or `"cfapi"`. Null if the VFS layer is not running. |
| `capabilities` | string[] | Advertised query capabilities currently active on this node. Values are well-known strings: `"search"` indicates the node can service search queries. Updated dynamically by the agent as plugins come online and go offline — a socket plugin that disconnects removes its capability until it reconnects. The control plane uses this field to route `POST /api/query` requests; the UI uses it to discover what query types are available without knowing which specific plugins are installed. |
| `transfer.endpoint` | string | Host:port for direct P2P file transfer. |
| `transfer.protocol` | string | Always `"http"` in v1. |
| `storage` | array? | Physical nodes only. Array of filesystem entries, one per filesystem containing watched paths. Refreshed hourly by the agent. |
| `storage[].filesystem_id` | string | Stable identifier for this filesystem. UUID from `blkid` on Linux, `diskutil` on macOS. |
| `storage[].mount_point` | string | Mount point of the filesystem, e.g. `"/"` or `"/mnt/data"`. |
| `storage[].fs_type` | string | Filesystem type: `"ext4"`, `"xfs"`, `"apfs"`, `"zfs"`, `"ntfs"`, etc. |
| `storage[].device` | string | Block device path, e.g. `"/dev/sda1"` or `"/dev/mapper/vg0-root"`. |
| `storage[].capacity_bytes` | uint64 | Total capacity of the filesystem in bytes. |
| `storage[].used_bytes` | uint64 | Used bytes at last agent refresh. Current snapshot figures live in `utilization_snapshot`. |
| `storage[].watch_paths_on_fs` | string[] | MosaicFS watch paths that reside on this filesystem. |
| `storage[].volume` | object? | Present when the filesystem sits on a logical volume. Contains `type` (`"lvm"`, `"zfs"`, `"apfs_container"`), volume group or pool name, logical volume name, and VG/pool total and free bytes. |
| `storage[].disk` | object? | Present when the underlying physical disk is identifiable. Contains device path, vendor, model, serial number, capacity in bytes, and interface type (`"nvme"`, `"sata"`, `"usb"`, etc.). |
| `network_mounts` | array? | Physical nodes only. Records network and cloud filesystems already mounted locally on this node. Used by the VFS layer's tiered access system to avoid redundant data transfer. Managed via the API; not collected automatically by the agent. |
| `network_mounts[].mount_id` | string | Short random identifier for this mount entry. Used by the API to target a specific mount for update or deletion. |
| `network_mounts[].remote_node_id` | string | The MosaicFS node whose files are accessible via this mount. |
| `network_mounts[].remote_base_export_path` | string | The base export path on the remote node that this mount covers. Matched against `source.export_path` values when the VFS layer resolves tiered access. |
| `network_mounts[].local_mount_path` | string | The local path at which the remote filesystem is mounted on this node. |
| `network_mounts[].mount_type` | string | `"cifs"`, `"nfs"`, `"gdrive_local"`, `"icloud_local"`, etc. |
| `network_mounts[].priority` | int | Higher values preferred when multiple mounts could serve the same file. |

**Example virtual directory** — a "Recent work documents" directory that mounts `.pdf`, `.docx`, and `.md` files from the laptop's documents folder, modified within the last 90 days, excluding anything under an `archive` subdirectory, but always including files with `URGENT` in the name. The parent `/documents` directory has `enforce_steps_on_children: true` with a step that excludes `.tmp` files globally — this is automatically prepended to the evaluation here.

```json
{
  "_id": "dir::sha256(/documents/work)",
  "type": "virtual_directory",
  "virtual_path": "/documents/work",
  "name": "work",
  "parent_path": "/documents",
  "inode": 4821,
  "created_at": "2025-11-14T09:00:00Z",
  "enforce_steps_on_children": false,
  "mounts": [
    {
      "mount_id": "a3f9",
      "source": { "node_id": "node-laptop", "export_path": "/home/user/documents" },
      "strategy": "prefix_replace",
      "source_prefix": "/home/user/documents",
      "steps": [
        { "op": "glob",  "pattern": "**/*.{pdf,docx,md}" },
        { "op": "glob",  "pattern": "**/archive/**", "invert": true },
        { "op": "regex", "pattern": "URGENT", "on_match": "include" },
        { "op": "age",   "max_days": 90 }
      ],
      "default_result": "include",
      "conflict_policy": "last_write_wins"
    }
  ]
}
```

When the VFS layer evaluates this directory, the parent's `.tmp` exclusion step is prepended first, then the mount's own steps run in sequence. Step 3 short-circuits any file with `URGENT` in the name directly to `include`, bypassing the age check. Files passing all steps without short-circuiting are included by `default_result`.

### Credential Document

Stores authentication credentials for agents and the web UI. Secret keys are stored as Argon2id hashes and are never recoverable after creation.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"credential::{access_key_id}"`. |
| `type` | string | Always `"credential"`. |
| `access_key_id` | string | Public identifier, format: `"MOSAICFS_{16_hex_chars}"`. Safe to log. |
| `secret_key_hash` | string | Argon2id hash of the secret key. Format: `"argon2id:$argon2id$..."`. |
| `name` | string | Human-readable label, e.g. `"Main laptop agent"`. |
| `enabled` | bool | Disabled credentials are rejected. |
| `created_at` | string | ISO 8601 creation timestamp. |
| `last_seen` | string? | ISO 8601 timestamp of last successful authentication. |
| `permissions.scope` | string | Always `"full"` in v1. Reserved for future scoped permissions. |

### Agent Status Document

Published by each agent on a regular schedule. Provides a rich operational picture of each node for the web UI status dashboard.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"status::{node_id}"`. |
| `type` | string | Always `"agent_status"`. |
| `node_id` | string | The node this status document describes. |
| `updated_at` | string | ISO 8601 timestamp of last update. |
| `overall` | string | `"healthy"`, `"degraded"`, or `"unhealthy"`. |
| `subsystems` | object | Per-subsystem status objects: `crawler`, `watcher`, `replication`, `cache`, `transfer`. |
| `recent_errors` | array | Last 50 errors, each with `time`, `subsystem`, `level`, and `message` fields. |

### Utilization Snapshot Document

A point-in-time record of storage capacity and usage, written hourly by each agent. Snapshots are never updated in place — each hour produces a new document. This makes utilization history queryable using CouchDB key-range queries on the timestamp component of the `_id`, and means there is no contention between the agent writing snapshots and the control plane or web UI reading them. Snapshots older than 90 days are pruned by a background task on the control plane.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"utilization::{node_id}::{ISO8601_timestamp}"`. Timestamp truncated to the hour, e.g. `"utilization::node-laptop::2025-11-14T09:00:00Z"`. |
| `type` | string | Always `"utilization_snapshot"`. |
| `node_id` | string | The node this snapshot describes. |
| `captured_at` | string | ISO 8601 timestamp when the snapshot was taken. |
| `filesystems` | array? | Physical nodes only. One entry per filesystem. |
| `filesystems[].filesystem_id` | string | Matches the `filesystem_id` in the node document `storage` array. Used to join snapshots to their filesystem topology. |
| `filesystems[].mount_point` | string | Mount point at time of capture. Included for readability; `filesystem_id` is the stable join key. |
| `filesystems[].used_bytes` | uint64 | Bytes used on this filesystem at time of capture. |
| `filesystems[].available_bytes` | uint64 | Bytes available at time of capture. Note: used + available may be less than capacity due to reserved blocks. |
| `cloud` | object? | Source-mode storage backends only. Present when the backend has cloud storage metadata. |
| `cloud.used_bytes` | uint64 | Bytes consumed in the cloud service at time of capture. |
| `cloud.quota_bytes` | uint64? | Total quota in bytes. Omitted for consumption-billed services (S3, B2). |

---

### Label Assignment Document

Associates one or more user-defined labels with a specific file. Created and updated via the API; the agent crawler never reads or writes this document type. Label assignments survive file re-indexing — if a file is modified and its `file` document is rewritten by the crawler, the corresponding `label_assignment` document is unaffected.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"label_file::{file_uuid}"`. One document per file. Labels attach to the file's identity, not to a location — they survive migration without changes. |
| `type` | string | Always `"label_assignment"`. |
| `file_id` | string | The full `_id` of the file document (e.g. `"file::a3f9..."`). Stored for convenience. |
| `labels` | string[] | Ordered array of label strings. Labels are arbitrary user-defined strings. No central registry — a label exists when something references it. |
| `updated_at` | string | ISO 8601 timestamp of last modification. |
| `updated_by` | string | Access key ID of the credential that last wrote this document. |

### Label Rule Document

Applies one or more labels to all files whose `source.export_path` starts with a given prefix on a given node. This is the mechanism behind "apply labels to all files in this folder and its subdirectories." A label rule is a declaration, not a bulk write — it does not modify individual file documents. The rule engine and search API compute a file's effective label set at query time by taking the union of its `label_assignment` labels and all `label_rule` labels whose `path_prefix` covers the file's `export_path`.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"label_rule::{uuid}"`. UUID assigned at creation. |
| `type` | string | Always `"label_rule"`. |
| `node_id` | string | ID of the node this rule applies to. May be `"*"` to apply to files from all nodes (useful for cross-node label rules). |
| `path_prefix` | string | Path prefix to match against `source.export_path`. A file matches if its path starts with this prefix. Must end with `/` to avoid partial directory name matches (e.g. `"/home/mark/documents/"` not `"/home/mark/documents"`). |
| `labels` | string[] | Labels to apply to all matching files. |
| `name` | string | Human-readable description shown in the web UI (e.g. `"Work documents"`). |
| `enabled` | bool | Disabled rules are ignored by the rule engine and search API. |
| `created_at` | string | ISO 8601 creation timestamp. |

**Effective label set.** Given a file document, the effective label set is the union of its direct label assignments and all matching label rules:

```
effective_labels(file)
  → result = {}
  → fetch label_assignment where _id = "label_file::{file.uuid}"
      if found: result ∪= assignment.labels
  → fetch all label_rules where node_id IN [file.source.node_id, "*"] AND enabled = true
      for each rule where file.source.export_path starts with rule.path_prefix:
          result ∪= rule.labels
  → return result
```

### Materialized Label Cache

Computing effective labels on every `readdir` call requires a per-file JOIN across `label_assignment` and `label_rule` documents — O(R) rule prefix comparisons per file, where R is the number of label rules for the node. At the target scale (500K files, 200 rules), this is measurable during directory listings with many files. The materialized label cache eliminates this cost by precomputing effective label sets in memory.

**Data structure.** Each agent maintains an in-memory hash map:

```
label_cache: HashMap<file_uuid, HashSet<String>>
```

The cache holds the effective label set for every file that has at least one label (from either a direct assignment or a matching rule). Files with no effective labels are not stored — an absent key means the empty set. At 500K files with 10% having labels, the cache holds ~50K entries, consuming roughly 5–10 MB of memory.

**Initial build.** On agent startup, after the local CouchDB replica is ready:

```
build_label_cache()
  → load all label_assignment documents from local replica
  → load all enabled label_rule documents from local replica
  → for each label_assignment:
      cache[assignment.file_uuid] ∪= assignment.labels
  → for each label_rule:
      query all active file documents where source.export_path starts with rule.path_prefix
        AND source.node_id = rule.node_id (or rule.node_id = "*")
      for each matching file:
        cache[file.uuid] ∪= rule.labels
```

The initial build runs once at startup and completes before the VFS mount becomes available. At 500K files and 200 rules, this takes a few seconds — dominated by the CouchDB prefix queries for rules.

**Incremental maintenance.** The agent watches the local CouchDB changes feed for three document types and updates the cache incrementally:

| Change | Cache action |
|---|---|
| `label_assignment` created/updated | Recompute entry for `assignment.file_uuid`: union of assignment labels + all matching rule labels. |
| `label_assignment` deleted | Recompute entry: matching rule labels only. Remove entry if result is empty. |
| `label_rule` created/updated/enabled | For all active files matching `(rule.node_id, rule.path_prefix)` against `(file.source.node_id, file.source.export_path)`: add `rule.labels` to each entry. |
| `label_rule` deleted/disabled | For all files matching the rule's scope: recompute from scratch (re-evaluate all remaining rules + direct assignment). |
| `file` created | Compute effective labels for the new file. If non-empty, insert entry. |
| `file` deleted | Remove entry. |
| `file` modified (path change) | Remove old entry, compute and insert new entry. |

Rule changes that affect many files (a broad prefix rule being added or removed) trigger a batch recomputation. The agent processes these asynchronously — the readdir cache TTL (default 5 seconds) means a brief window where the old label set is still served, which is acceptable.

**Usage in readdir.** The rule engine's step pipeline replaces the per-file CouchDB query with a hash map lookup:

```
resolve effective_labels(file)
  → return label_cache.get(file.uuid)
       .unwrap_or(empty_set)
```

This is O(1) per file regardless of the number of label rules.

**Usage in search.** The control plane's search API also benefits from the label cache. When the search endpoint filters by label, it can check the cache rather than joining against label documents for each candidate file. The control plane maintains its own label cache instance, built from the central CouchDB.

**Why annotations are not cached.** Annotations are loaded lazily during step evaluation — only for files that survive prior filter steps, and only for the specific `plugin_name` referenced in the `annotation` step op. The access pattern is sparse and already indexed. Including annotations in the cache would increase memory usage substantially (annotation `data` objects are arbitrarily large), cause frequent cache churn during plugin processing, and provide minimal benefit since the lazy evaluation already limits the number of lookups. Labels and annotations have fundamentally different access patterns: labels are checked for every file on every readdir; annotations are checked for a small subset of files that reach an annotation step.

### Access Tracking Document

Records the most recent time a file was accessed through MosaicFS. One document per file — not per node, because the system tracks "when was this file last used by anyone" rather than per-node access history. Written by whichever agent served the access, with debouncing to limit replication churn.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"access::{file_uuid}"`. The `file_uuid` is the UUID portion of the file document's `_id` (e.g. `file::a3f9...` → `a3f9...`). Using the UUID rather than the full file `_id` keeps access documents short and consistent. |
| `type` | string | Always `"access"`. |
| `file_id` | string | The full `_id` of the file document (e.g. `"file::a3f9..."`). Stored for convenience — avoids requiring callers to reconstruct the file `_id` from the UUID. |
| `source.node_id` | string | The node ID of the agent that wrote this document. Used by the replication push filter. This is the node that *served* the access, which may differ from the node that *owns* the file (e.g. a VFS node accessing a remote file records the access locally, then pushes the document). |
| `last_access` | string | ISO 8601 timestamp of the most recent access. Updated only if the previous value is older than the debounce threshold (default 1 hour). |
| `access_count` | int | Running count of accesses observed since the document was created. Incremented on each debounced flush, not on every individual access — so this represents the number of flush cycles that observed at least one access, not the true access count. Useful for rough "hot file" identification. |

**Capture points.** Access is recorded at four points in the system, all feeding the same in-memory tracker:

| Access Path | Where Recorded | Semantics |
|---|---|---|
| VFS/FUSE `open()` | FUSE backend on the VFS node | A local user opened a file through the virtual filesystem. |
| REST API `GET /api/files/{file_id}/content` | Axum handler on the control plane | A browser or CLI client downloaded file content. |
| Agent transfer (Tier 4 requester) | VFS cache layer on the requesting agent | An agent fetched a remote file for local access. The *requesting* agent records the access, not the serving agent — the access semantically belongs to the node where the user is. |
| Plugin materialize (Tier 5) | Agent handling the materialize event | A source-mode storage backend plugin extracted a file from external storage. |

All four paths call the same function — `access_tracker.record(file_id)` — which updates an in-memory map. No I/O occurs on the hot path.

**Debounced persistence.** The agent runs a background flush task:

- **Flush interval**: every 5 minutes (configurable via `agent.toml` as `access_tracking.flush_interval_s`, default 300).
- **Write threshold**: a CouchDB document is written only if the recorded `last_access` in the database is older than the debounce threshold (default 1 hour, configurable as `access_tracking.debounce_threshold_s`). A file opened 50 times in an hour produces one document write, not 50.
- **Batch writes**: all dirty entries are flushed in a single `_bulk_docs` request.
- **Graceful shutdown**: the tracker flushes on agent stop to minimize data loss.

At the target scale (500K files), the worst case is 500K / debounce_hours document writes per flush interval — but in practice most files are not accessed in any given hour, so flush batches are small.

**Orphan cleanup.** When the agent observes a file document transition to `status: "deleted"` via the changes feed, it deletes the corresponding access document in the next flush cycle. Access documents for files that no longer exist are harmless (they are simply never queried) but cleaning them up avoids unbounded growth.

---

### Materialized Access Cache

The `access_age` step pipeline operation needs to look up a file's last access time during readdir evaluation. Querying CouchDB per file would be too expensive — the same O(R) problem that motivates the materialized label cache. The access cache eliminates this cost.

**Data structure.** Each agent maintains an in-memory hash map:

```
access_cache: HashMap<String, DateTime<Utc>>
```

The key is the `file_id` string (e.g. `"file::a3f9..."`). The value is the `last_access` timestamp. Files with no access document are not stored — an absent key means "never accessed through MosaicFS."

**Memory cost.** At 500K files with 10% having been accessed through MosaicFS, the cache holds ~50K entries. Each entry is a string key (~40 bytes) plus a timestamp (8 bytes), totaling roughly 3–5 MB.

**Initial build.** On agent startup, after the local CouchDB replica is ready:

```
build_access_cache()
  → load all access documents from local replica
  → for each access document:
      cache[access.file_id] = access.last_access
```

**Incremental maintenance.** The agent watches the local CouchDB changes feed for `access` documents:

| Change | Cache action |
|---|---|
| `access` created/updated | `cache[doc.file_id] = doc.last_access` |
| `access` deleted | Remove entry for `doc.file_id` |

The local access tracker's flush also updates the in-memory cache directly (before writing to CouchDB), so the cache reflects local accesses within one flush interval even before replication round-trips.

**Multiple VFS nodes.** When the same file is accessed from different nodes, each node writes its own access document update. After replication, the access cache on every node reflects the most recently replicated `last_access` timestamp. The cache uses a simple overwrite — `max(current, incoming)` is not needed because the debounce threshold ensures that only genuinely newer timestamps produce document updates.

**Usage in readdir.**

```
resolve last_access(file)
  → return access_cache.get(file._id)
```

Returns `None` if the file has never been accessed through MosaicFS. The `access_age` step operation uses this to decide whether the file matches.

### Storage Backend Document

Defines an external storage service connection. Storage backends are the unified abstraction for all external storage — S3 buckets, B2 buckets, local directories, Google Drive, OneDrive, iCloud, and other MosaicFS agents. A backend operates in source mode (indexing files into MosaicFS), target mode (receiving file replicas), or bidirectional mode. Managed via the control plane API and web UI. The agent watches the changes feed and reloads backend configurations live.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"storage_backend::{name}"`. Deterministic — one document per backend. |
| `type` | string | Always `"storage_backend"`. |
| `name` | string | Human-readable identifier, unique across the system. Used as the key in replication rules and replica documents. e.g. `"offsite-backup"`, `"google-drive-main"`, `"icloud-photos"`. |
| `hosting_node_id` | string? | When set, only this agent runs the backend — required for platform-locked services (iCloud on macOS) or local directories. When omitted or `null`, any agent can talk to the service directly (S3, B2). |
| `backend` | string | The storage backend type. Determines which storage backend plugin handles I/O. Values: `"s3"`, `"b2"`, `"directory"`, `"agent"`, `"google_drive"`, `"onedrive"`, `"icloud"`. |
| `mode` | string | `"source"` (index files from the service into MosaicFS), `"target"` (replicate MosaicFS files to the service), or `"bidirectional"`. |
| `backend_config` | object | Backend-specific configuration. See below. |
| `credentials_ref` | string? | Reference to a credential or plugin setting containing the authentication material. Not used for `directory` or `agent` backends. |
| `schedule` | string? | Target mode: upload time window in `HH:MM-HH:MM` format. Source mode: poll schedule. |
| `poll_interval_s` | int? | Source mode: how often to scan for changes (seconds). |
| `bandwidth_limit_mbps` | int? | Optional upload bandwidth cap in megabits per second. Enforced by the agent via a token bucket rate limiter shared across all concurrent uploads to this backend. |
| `retention.keep_deleted_days` | int | How many days to retain a file on the backend after the source file is deleted from MosaicFS. `0` means delete from the backend immediately on `file.deleted`. |
| `remove_unmatched` | bool | Controls behavior when a previously replicated file no longer matches any replication rule for this backend. `false` (default): the replica is preserved but no longer maintained (status becomes `"frozen"`). `true`: the replica is moved to the deletion log and purged after the retention window. |
| `cloud_storage` | object? | Source mode: billing and quota metadata. Contains `billing_model` (`"quota"` or `"consumption"`), `quota_bytes` (uint64, present for quota-billed services), and `quota_available` (bool). |
| `enabled` | bool | When `false`, no new replication work is dispatched for this backend. Existing replicas are preserved. |
| `created_at` | string | ISO 8601 timestamp. |

**Backend-specific configuration:**

| Backend | `backend_config` Fields |
|---|---|
| `s3` | `bucket`, `prefix`, `region`, `storage_class` (optional, e.g. `"GLACIER"`) |
| `b2` | `bucket`, `prefix` |
| `directory` | `path` (absolute path on the local filesystem; must be in the agent's `excluded_paths` to prevent crawl indexing) |
| `agent` | `node_id` (destination agent), `path_prefix` (where to write on the destination; must be in that agent's `excluded_paths` and `transfer_serve_paths`) |
| `google_drive` | `folder_id` (root folder to index), `include_shared` (bool) |
| `onedrive` | `drive_id`, `folder_path` |
| `icloud` | `sync_directory` (local path to `~/Library/Mobile Documents/`) |

---

### Replication Rule Document

Defines which files should be replicated to a specific target. Uses the same step pipeline operations as virtual directory mount sources — the replication subsystem evaluates rules using the same engine and caches (materialized label cache, materialized access cache) that the VFS readdir evaluation uses.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"repl_rule::{uuid}"`. UUID assigned at creation. |
| `type` | string | Always `"replication_rule"`. |
| `name` | string | Human-readable name for this rule. Displayed in the web UI and included in notifications. |
| `target_name` | string | The `name` of the storage backend this rule replicates to. |
| `source.node_id` | string | Filter to files from a specific node, or `"*"` for all nodes. |
| `source.path_prefix` | string? | Optional path prefix filter. Only files whose `export_path` starts with this prefix are considered. Must end with `/`. |
| `steps` | array | Step pipeline operations — same syntax and semantics as virtual directory mount steps. All ten ops are supported: `glob`, `regex`, `age`, `size`, `mime`, `node`, `label`, `access_age`, `replicated`, `annotation`. |
| `default_result` | string | `"include"` or `"exclude"`. Applied when no step short-circuits. |
| `enabled` | bool | When `false`, the rule is not evaluated. |
| `created_at` | string | ISO 8601 timestamp. |
| `updated_at` | string | ISO 8601 timestamp. |

A file may match multiple rules targeting different (or the same) targets. Each match produces an independent replication action. The step pipeline evaluation is identical to virtual directory readdir evaluation — same logic, same caches, same short-circuit semantics.

---

### Replica Document

Records the fact that a copy of a specific file exists on a specific replication target. Written by the agent's replication subsystem after a successful upload via a storage backend plugin. Read by the VFS layer for Tier 4b failover and by the `replicated` step pipeline operation.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"replica::{file_uuid}::{target_name}"`. Deterministic — one document per (file, target) pair. The `file_uuid` is the UUID portion of the file document's `_id` (e.g. `file::a3f9...` → `a3f9...`). |
| `type` | string | Always `"replica"`. |
| `file_id` | string | The full `_id` of the source file document (e.g. `"file::a3f9..."`). Stored for convenience and indexing. |
| `target_name` | string | The storage backend where this replica is stored. |
| `source.node_id` | string | The `node_id` of the agent that wrote this document — the agent that performed the upload. Used by the push replication filter. |
| `backend` | string | The storage backend type (copied from the target document for fast lookup during Tier 4b). |
| `remote_key` | string | The key or path used to store the file on the target. Backend-specific: S3 object key, B2 file name, local filesystem path, or export path on the destination agent. Used by the VFS and restore operations to locate the replica. |
| `replicated_at` | string | ISO 8601 timestamp of the most recent successful upload. |
| `source_mtime` | string | The file's `mtime` at the time of replication. Used to detect staleness — if the file's current `mtime` differs, the replica is stale. |
| `source_size` | int | The file's `size` at the time of replication. Used alongside `source_mtime` for staleness detection. |
| `checksum` | string? | SHA-256 checksum of the replicated content, if provided by the storage backend plugin. |
| `status` | string | `"current"`, `"stale"`, or `"frozen"`. See below. |

**Replica status values:**

| Status | Meaning |
|---|---|
| `"current"` | The replica matches the source file's mtime/size. The target copy is up to date. |
| `"stale"` | The source file has been modified since the last replication. The agent will re-upload on the next replication cycle. |
| `"frozen"` | The file no longer matches any replication rule for this target (and `remove_unmatched` is `false`). The replica is preserved on the target but not actively maintained — future source modifications will not be synced. |

**Staleness detection.** When the agent's replication subsystem observes a `file.modified` event, it checks all replica documents for that file. Any replica whose `source_mtime` or `source_size` no longer matches the file document is updated to `status: "stale"`. The agent then schedules a re-upload.

**Lifecycle.** When the agent's replication subsystem observes a `file.deleted` event, it handles each replica based on the target's `retention.keep_deleted_days`:
- `0`: delete the replica from the target immediately (via storage backend plugin), then delete the replica document.
- `> 0`: update the replica document with `status: "pending_deletion"` and a `delete_after` timestamp. A background sweep purges expired replicas.

**Remote key scheme.** Files are stored on the target using a deterministic key:

```
{prefix}/{file_uuid_8}/{filename}
```

The `file_uuid_8` is the first 8 characters of the file's UUID. This provides distribution on object stores while keeping filenames human-readable. The full file metadata is recoverable from the file document via `file_id`.

---

### Plugin Document

Configures one plugin on one agent node. Created and managed via the web UI and REST API. The agent watches the CouchDB changes feed for documents matching its own `node_id` and reloads plugin configuration live — no restart required. Changes to a plugin document while jobs are in flight complete with the previous configuration; the updated configuration takes effect for subsequent jobs.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"plugin::{node_id}::{plugin_name}"`. Deterministic — one document per plugin per node. |
| `type` | string | Always `"plugin"`. |
| `node_id` | string | ID of the agent node this plugin runs on. |
| `plugin_name` | string | Name of the plugin executable. Resolved to the platform-specific plugin directory at invocation time. Must match an executable present in that directory; if the binary is absent, jobs fail immediately with a permanent error. |
| `plugin_type` | string | `"executable"` or `"socket"`. Determines the invocation model. |
| `enabled` | bool | Disabled plugins receive no events and enqueue no jobs. |
| `name` | string | Human-readable display name shown in the web UI (e.g. `"AI Document Summariser"`). |
| `subscribed_events` | string[] | Events this plugin receives. Valid values: `"file.added"`, `"file.modified"`, `"file.deleted"`, `"access.updated"` (emitted when the agent flushes a debounced access document update — useful for plugins that react to file usage patterns), `"sync.started"`, `"sync.completed"`, `"crawl_requested"` (source-mode backend agents only), `"materialize"` (source-mode backend agents only), `"replica.upload"`, `"replica.download"`, `"replica.delete"`, `"replica.list"`, `"replica.health"` (storage backend plugins only — dispatched by the agent's replication subsystem, not by the plugin runner's normal event dispatch). A plugin that does not subscribe to an event type never receives it. The API does not reject event subscriptions that cannot fire on a given node type — the events simply never fire, so the subscription is harmless. |
| `mime_globs` | string[] | Optional MIME type filter. If non-empty, only files whose `mime_type` matches at least one glob are enqueued. e.g. `["application/pdf", "text/*"]`. Files with no `mime_type` do not match. |
| `config` | object | Arbitrary JSON object passed to the plugin in the `config` field of every event's stdin payload. The plugin reads whatever keys it needs; extra keys are ignored. |
| `workers` | int | Number of concurrent workers for this plugin. Default 2. For executable plugins, this is the number of simultaneous child processes. Socket plugins use a single connection with a sliding acknowledgement window. |
| `timeout_s` | int | Maximum seconds to wait for a plugin response before treating the invocation as failed. Default 60. |
| `max_attempts` | int | Maximum number of attempts before a job is moved to permanent failure state. Default 3. Does not apply to socket plugins — delivery is retried indefinitely via the ack queue until the socket reconnects. |
| `query_endpoints` | array? | Optional. Declares query endpoints this plugin handles. Each entry causes the agent to advertise a capability on the node document and accept query requests routed by the control plane. See query endpoint fields below. |
| `query_endpoints[].name` | string | Internal endpoint name, used in routing. e.g. `"search"`. |
| `query_endpoints[].capability` | string | The well-known capability string this endpoint satisfies. Defined capability values: `"search"` — the plugin handles text search queries dispatched by the browser via `POST /api/query`; `"dashboard_widget"` — the plugin provides a periodic health/status summary polled by the control plane and displayed as a widget card on the dashboard. Multiple plugins on the same node may advertise the same capability — their results are merged in the response. |
| `query_endpoints[].description` | string | Human-readable description shown in the UI. e.g. `"Full-text search powered by Meilisearch"`. |
| `settings_schema` | object? | Optional. A JSON Schema-subset object declaring the user-configurable settings for this plugin. When present, the web UI renders a settings form on the Settings page rather than a raw JSON editor. Each property in `settings_schema.properties` describes one field with `type` (`"string"`, `"number"`, `"boolean"`, `"enum"`), `title` (label), `description` (help text), `default`, and for enum fields an `enum` array of permitted values. A `"secret"` type is also supported — rendered as a password input, displayed as `••••••••` after save. Required fields are listed in `settings_schema.required`. |
| `settings` | object? | User-provided values for the fields declared in `settings_schema`. Written by the web UI when the user saves the settings form. The agent merges `settings` into `config` at invocation time — the plugin binary receives a single flat `config` object and does not need to know which keys came from `settings` and which from `config`. If a key appears in both, `settings` takes precedence. |
| `provides_filesystem` | bool? | Default `false`. When `true`, this plugin acts as the filesystem for the node — the agent invokes it for crawl events instead of walking real watch paths, and for materialize events when the transfer server needs to serve a file whose `export_path` falls under `file_path_prefix`. Only meaningful on agents hosting source-mode storage backends where `agent.toml` declares no watch paths. |
| `file_path_prefix` | string? | Required when `provides_filesystem` is `true`. Export path prefix identifying files owned by this plugin, e.g. `"/gmail"`. The transfer server checks whether a requested file's `export_path` starts with this prefix to decide whether to invoke the plugin's materialize action. Must be unique across all plugins on a given node. |
| `created_at` | string | ISO 8601 creation timestamp. |

**Plugin directory paths by platform:**

| Platform | Path |
|---|---|
| Linux | `/usr/lib/mosaicfs/plugins/` |
| macOS | `/Library/Application Support/MosaicFS/plugins/` |
| Windows | `C:\ProgramData\MosaicFS\plugins\` |

The agent enumerates this directory at startup and after each `inotify`/`FSEvents` change to the directory, reporting available plugin names in `agent_status.available_plugins`. The web UI uses this list to populate the plugin name dropdown when creating a plugin configuration.

**Event envelope (stdin for executable plugins; framed JSON over socket for socket plugins):**

The agent merges `settings` into `config` before constructing the envelope — `settings` values take precedence over same-named keys in `config`. The plugin binary receives a single flat `config` object and does not need to distinguish between the two sources.

```json
{
  "event":      "file.added",
  "sequence":   1042,
  "timestamp":  "2026-02-16T09:22:00Z",
  "node_id":    "node-laptop",
  "payload": {
    "file_id":      "file::a3f9...",
    "export_path":  "/home/mark/documents/report.pdf",
    "name":         "report.pdf",
    "size":         204800,
    "mime_type":    "application/pdf",
    "mtime":        "2026-01-15T14:30:00Z"
  },
  "config": {
    "meilisearch_url": "http://meilisearch:7700",
    "meilisearch_api_key": "abc123",
    "max_results": 20
  }
}
```

**Example `settings_schema` and `settings` for the fulltext-search plugin:**

```json
"settings_schema": {
  "properties": {
    "meilisearch_url": {
      "type": "string",
      "title": "Meilisearch URL",
      "description": "URL of the Meilisearch instance, e.g. http://meilisearch:7700",
      "default": "http://meilisearch:7700"
    },
    "meilisearch_api_key": {
      "type": "secret",
      "title": "Meilisearch API Key",
      "description": "Master key or search API key. Leave blank if authentication is disabled."
    },
    "max_results": {
      "type": "number",
      "title": "Maximum results per query",
      "default": 20
    }
  },
  "required": ["meilisearch_url"]
},
"settings": {
  "meilisearch_url": "http://meilisearch:7700",
  "meilisearch_api_key": "abc123",
  "max_results": 20
}
```

For `sync.started` and `sync.completed`, `payload` contains `{ "trigger": "manual" | "scheduled" }` and no file fields. For `file.deleted`, `payload` contains the file's last-known metadata.

**Source-mode backend events (`provides_filesystem: true` plugins only):**

Two additional event types are delivered exclusively to filesystem-providing plugins on agents hosting source-mode backends.

`crawl_requested` — delivered by the agent on startup, on the nightly crawl schedule, and when the user triggers a manual sync. The plugin fetches new data from its external source, writes files to its `backend-data/files/` directory, and returns a list of file operations for the agent to apply to CouchDB. `payload` contains `{ "trigger": "startup" | "scheduled" | "manual" }`.

Plugin stdout for `crawl_requested`:
```json
{
  "files": [
    {
      "action": "create",
      "export_path": "/gmail/2026/02/16/re-project-kickoff.eml",
      "size": 45231,
      "mtime": "2026-02-16T09:15:00Z",
      "mime_type": "message/rfc822"
    },
    {
      "action": "delete",
      "export_path": "/gmail/2026/01/10/old-newsletter.eml"
    }
  ]
}
```

The agent processes this list and applies it to CouchDB via `_bulk_docs` — creating, updating, or soft-deleting file documents as indicated. Files listed as `create` that already have a current document (same `mtime` and `size`) are skipped. This makes the crawl response idempotent: the plugin can safely return the full set of known files rather than only the delta.

`materialize` — delivered by the agent's transfer server when a file under `file_path_prefix` is requested and not present in the VFS cache. The plugin extracts the file from its internal storage (SQLite database, API response, etc.) and writes it to a staging path provided by the agent. The agent then takes over: moves the staged file into the VFS cache, inserts a cache index entry, and serves the bytes using the standard path. The plugin is responsible only for writing the bytes to disk — all cache management, integrity checking, and streaming is handled by the agent.

`materialize` stdin payload:
```json
{
  "event":        "materialize",
  "file_id":      "file::abc123",
  "export_path":  "/gmail/2026/02/16/re-project-kickoff.eml",
  "staging_path": "/var/lib/mosaicfs/cache/tmp/plugin-abc123",
  "config":       { }
}
```

Plugin stdout for `materialize`:
```json
{ "size": 45231 }
```

The plugin writes the file bytes to `staging_path` and returns the byte count. If materialization fails (message deleted from external source, authentication error), it exits non-zero with an error message on stderr. The agent logs the failure, removes the partial staging file, and returns a 503 to the requester. No retry — the next access will attempt materialization again.

**Executable plugin stdout contract:**

The plugin writes a single JSON object to stdout and exits 0 for success, non-zero for failure. Any top-level key in the returned object is written into the `annotation` document's `data` field. If the plugin has nothing to write back (it updated an external system), it returns `{}`. Malformed JSON or a non-zero exit is treated as a transient failure and retried up to `max_attempts`.

```json
{ "summary": "Quarterly earnings report for Q3 2025.", "language": "en" }
```

**Executable plugin invocation contract:**

The agent invokes the plugin binary directly via `execv`, not via a shell. The complete invocation environment is:

| Aspect | Value |
|---|---|
| Working directory | The agent's state directory (`/var/lib/mosaicfs/` on Linux, `~/Library/Application Support/MosaicFS/` on macOS). Plugins that need persistent state should write to a subdirectory named after themselves under this path. |
| User/group | Same as the agent process. On Linux this is typically `root` (required for FUSE). Plugins inherit these privileges. |
| Stdin | A single JSON object (the event envelope), followed by EOF. |
| Stdout | A single JSON object (the response), followed by EOF. Maximum 10 MB — responses exceeding this are treated as a permanent error. |
| Stderr | Free-form text, captured by the agent and written to the agent log at `WARN` level. Stderr is not parsed. Maximum 1 MB captured; excess is discarded. |
| Exit code | 0 = success (stdout parsed as JSON response). Non-zero = failure (retried up to `max_attempts`). Exit code 78 (`EX_CONFIG`) is treated as a permanent error and not retried — used when the plugin detects a misconfiguration. |
| Timeout | Controlled by `timeout_s` on the plugin document (default 60s). On timeout, the process is sent `SIGTERM`, then `SIGKILL` after 5 seconds. Treated as a transient failure. |
| Environment | Inherits the agent's environment. No additional environment variables are set — all configuration is passed via the `config` field in the stdin payload. |
| Arguments | None. The binary is invoked with no command-line arguments. |
| File descriptors | Only stdin, stdout, and stderr. No additional file descriptors are passed. |

**Socket plugin ack protocol:**

The agent writes newline-delimited JSON events to the socket, each containing a `sequence` number. The plugin responds with newline-delimited JSON acks:

```json
{ "ack": 1042 }
```

The agent maintains a sliding window of unacknowledged events in the SQLite job queue. On socket disconnect, the agent retries the connection with exponential backoff and replays all unacknowledged events in sequence order after reconnecting. Socket plugins must be idempotent — they will receive duplicate events after a reconnect.

**Query invocation (executable plugins with `query_endpoints`):**

When the control plane routes a query to an agent, the agent invokes the plugin binary with the query payload on stdin and reads the result from stdout synchronously. This is request/response, not fire-and-forget. The plugin must respond within `timeout_s`.

Query stdin payload:
```json
{
  "query":    "quarterly earnings",
  "endpoint": "search",
  "config":   { }
}
```

Query stdout response — a result envelope identifying the plugin and containing an array of results:
```json
{
  "plugin_name":  "fulltext-search",
  "capability":   "search",
  "description":  "Full-text search powered by Meilisearch",
  "results": [
    {
      "file_id":   "file::a3f9...",
      "score":     0.94,
      "fragments": ["...quarterly **earnings** report for Q3..."]
    }
  ]
}
```

Each result may be a MosaicFS file reference (identified by `file_id`, looked up in PouchDB by the browser to display standard file metadata) or a free-form item (no `file_id`, rendered generically). The `fragments` field carries matched text snippets for search results. Additional fields are allowed and rendered as supplementary metadata.

**Dashboard widget response (`capability: "dashboard_widget"`):**

Polled by the control plane on a schedule rather than dispatched by the browser. The plugin returns a compact status summary:

```json
{
  "plugin_name":  "fulltext-search",
  "capability":   "dashboard_widget",
  "description":  "Full-text search powered by Meilisearch",
  "status":       "healthy",
  "data": {
    "Documents indexed": "47,203",
    "Index lag":         "0",
    "Last sync":         "2 minutes ago"
  }
}
```

`status` is `"healthy"`, `"warning"`, or `"error"` — controls the widget card's visual treatment. `data` is an ordered set of key-value pairs rendered as a compact list in the widget card. Values are strings; the plugin is responsible for human-readable formatting.

---

### Annotation Document

Structured metadata written back to CouchDB by an executable plugin. One document per `(file, plugin_name)` — rerunning the plugin for the same file overwrites the previous annotation. Socket plugins that update external systems (a search engine, an external database) typically produce no annotation documents; their output is their external system.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"annotation::{file_uuid}::{plugin_name}"`. Deterministic — one document per file per plugin. Keyed by file UUID, so annotations survive migration without changes. |
| `type` | string | Always `"annotation"`. |
| `file_id` | string | The full `_id` of the file document (e.g. `"file::a3f9..."`). Stored for convenience. |
| `source.node_id` | string | The node ID of the agent whose plugin runner wrote this annotation. Used by the push replication filter to scope outbound replication. |
| `plugin_name` | string | Name of the plugin that produced this annotation. Acts as the namespace — two plugins writing `summary` keys produce two separate annotation documents, not a collision. |
| `data` | object | The plugin's stdout JSON object, stored verbatim. Structure is defined entirely by the plugin. MosaicFS does not interpret or validate the contents. |
| `status` | string | `"ok"` or `"failed"`. Failed annotations are written when `max_attempts` is exhausted, preserving the failure record in the database. |
| `error` | string? | Present when `status` is `"failed"`. Last error message from the plugin invocation. |
| `annotated_at` | string | ISO 8601 timestamp when this annotation was last written. Compared against the file's `mtime` by the plugin runner to determine whether re-annotation is needed on a reconciliation crawl or full sync. |
| `updated_at` | string | ISO 8601 timestamp of last document modification. |

---

### Notification Document

A system event or condition requiring user attention. Written by agents, storage backends, the control plane, and plugins. Replicated to the browser via PouchDB for live delivery. The `_id` scheme deduplicates notifications so a recurring condition updates the existing document rather than accumulating duplicates.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"notification::{source_id}::{condition_key}"`. `source_id` is the node ID or `"control_plane"`. `condition_key` is a stable string identifying the condition type — e.g. `"oauth_expired"`, `"replication_lag"`, `"inotify_limit_approaching"`, `"plugin_jobs_failed:fulltext-search"`. Using a deterministic `_id` means a recurring condition performs an upsert rather than creating a new document. |
| `type` | string | Always `"notification"`. |
| `source.node_id` | string | ID of the node or `"control_plane"` that produced this notification. Used to route the "View details" action and to filter notifications by source in the UI. |
| `source.component` | string | The subsystem that produced the notification: `"crawler"`, `"watcher"`, `"replication"`, `"cache"`, `"storage_backend"`, `"plugin:{plugin_name}"`, `"control_plane"`, etc. Displayed alongside the source node badge in the UI. |
| `severity` | string | `"info"`, `"warning"`, or `"error"`. Controls visual treatment in the UI and sort order in the notification panel. |
| `status` | string | `"active"`, `"resolved"`, or `"acknowledged"`. Writers set `"active"` when the condition arises and `"resolved"` when it clears. The user sets `"acknowledged"` via the UI or REST API. An acknowledged notification that transitions back to `"active"` (condition recurred) un-acknowledges automatically — the `status` field is set to `"active"` again by the writer. |
| `title` | string | Short notification title shown in the notification bell panel and dashboard alert area. e.g. `"OAuth token expired"`, `"Meilisearch index lag"`. |
| `message` | string | Full human-readable description of the condition. Shown in the notification detail view. May include counts, paths, or specific error messages. e.g. `"Google Drive OAuth token expired on 2026-02-14. Re-authorization is required to resume syncing."` |
| `actions` | array? | Optional list of actions the user can take directly from the notification. Each action has a `label` string and an `api` field containing a REST API path to call when the button is clicked. e.g. `{ "label": "Re-authorize", "api": "GET /api/nodes/{node_id}/auth" }`. The UI renders these as buttons in the notification detail view. |
| `condition_key` | string | The stable condition identifier, extracted from `_id` for convenience. Used by writers to check whether an existing notification document exists before deciding to create or update. |
| `first_seen` | string | ISO 8601 timestamp when this condition was first observed. Not updated on subsequent occurrences — preserves the original onset time. |
| `last_seen` | string | ISO 8601 timestamp of the most recent occurrence or update. Updated on every write. |
| `occurrence_count` | int | Number of times this condition has been written since `first_seen`. Incremented on each upsert. Displayed in the UI as "47 occurrences since Feb 14" for high-frequency conditions. |
| `acknowledged_at` | string? | ISO 8601 timestamp when the user acknowledged this notification. Cleared when the notification transitions back to `"active"`. |
| `resolved_at` | string? | ISO 8601 timestamp when the condition resolved. Present only when `status` is `"resolved"`. |

**Condition keys by source:**

| Source | Condition Key | Severity | Auto-resolves |
|---|---|---|---|
| Agent crawler | `first_crawl_complete` | info | No (one-shot, stays resolved) |
| Agent crawler | `inotify_limit_approaching` | warning | Yes (clears when watches freed) |
| Agent watcher | `watch_path_inaccessible:{path}` | error | Yes |
| Agent replication | `replication_lag` | warning | Yes |
| Agent cache | `cache_near_capacity` | warning | Yes |
| Storage backend | `oauth_expired:{backend}` | error | Yes (clears on re-auth) |
| Storage backend | `oauth_expiring_soon:{backend}` | warning | Yes |
| Storage backend | `quota_near_limit:{backend}` | warning | Yes |
| Storage backend | `sync_stalled:{backend}` | error | Yes |
| Storage backend | `large_remote_deletion:{backend}` | warning | No (requires ack) |
| Plugin (executable) | `plugin_jobs_failed:{plugin_name}` | error | Yes (clears when queue drains) |
| Plugin (socket) | `plugin_disconnected:{plugin_name}` | warning | Yes (clears on reconnect) |
| Plugin (socket) | `plugin_health_check_failed:{plugin_name}` | warning | Yes |
| Plugin (any) | Arbitrary `condition_key` from health check response | Any | Plugin-controlled |
| Control plane | `new_node_registered:{node_id}` | info | No (requires ack) |
| Control plane | `credential_inactive:{key_id}` | warning | No (requires ack) |
| Control plane | `control_plane_disk_low` | warning | Yes |

**Plugin-issued notifications via health check:**

Socket plugins emit notifications in the health check response. The agent writes the notification document on the plugin's behalf — the plugin never writes to CouchDB directly. The plugin controls `condition_key`, `severity`, `title`, `message`, `actions`, and `resolve_notifications` (an array of `condition_key` values to mark resolved). The agent prefixes the `_id` with `notification::{node_id}::plugin:{plugin_name}:` to namespace plugin notifications under their source.

```json
{
  "status": "healthy",
  "notifications": [
    {
      "condition_key": "index_lag",
      "severity": "warning",
      "title": "Meilisearch index lag",
      "message": "Index is 1,847 documents behind. Last sync 4 minutes ago.",
      "actions": [
        { "label": "Trigger sync", "api": "POST /api/nodes/{node_id}/plugins/fulltext-search/sync" }
      ]
    }
  ],
  "resolve_notifications": ["index_lag_critical"]
}
``` Inode numbers are a concept native to FUSE; macOS File Provider and Windows CFAPI use analogous stable file identity tokens. The inode space is partitioned as follows:

| Range | Purpose |
|---|---|
| `0` | Reserved. FUSE treats 0 as invalid. |
| `1` | Root directory `"/"`. Stored in CouchDB as `_id "dir::root"`. |
| `2–999` | Reserved for future well-known synthetic entries. |
| `1000+` | Randomly assigned 64-bit integers, stored in the `inode` field of each document. |

The system is explicitly 64-bit only. A compile-time assertion in the Rust build script produces a human-readable error if compilation is attempted on a 32-bit platform.

---

