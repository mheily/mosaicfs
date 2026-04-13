<!-- MosaicFS Architecture · ../architecture.md -->

## VFS Tiered Access Strategy

**Lazy path resolution.** The VFS must never proactively stat,
enumerate, or health-check paths belonging to remote filesystems. A
remote mount is touched only when a user explicitly opens a file under
it. Eager traversal risks hanging NFS responses for the duration of the
OS timeout on every unavailable mount in the cluster. Reviewers should
reject any change that adds background probes against
`NetworkMount.local_mount_path` or `NodeAvailability.local_mount_path`.

When the VFS layer needs to open a file, it evaluates access tiers in order of increasing cost, stopping at the first available option. This logic lives in the common `mosaicfs-vfs` crate and is shared across all OS-specific backends:

All tiers record a file access via the access tracker (`access_tracker.record(file_id)`) — an in-memory update with no I/O on the hot path. See the Access Tracking Document section for details on debounced persistence.

**Tier 1 — Local file.** The file lives on this node. Open directly via the real path.

**Tier 2 — Network mount (CIFS/NFS).** The VFS looks up the `FilesystemDocument` for the filesystem owning this file. If this node has a matching `NodeAvailability` entry, it translates and opens via the local mount point recorded in that entry.

**Tier 3 — Local cloud sync directory.** The VFS looks up the `FilesystemDocument` for the filesystem owning this file. If this node has a matching `NodeAvailability` entry of type `icloud_local` or `gdrive_local`, it opens via the local sync directory, with eviction check for iCloud. If the file is evicted from local iCloud storage, fall through to replica failover rather than triggering an implicit cloud download.

**Replica failover.** Evaluated when no node-local access path is available. The VFS checks for available replicas of the file and attempts to fetch from one:

1. Query the local CouchDB replica for `replica` documents matching this file's `file_id` with `status` of `"current"` or `"frozen"`.
2. For each available replica, attempt to fetch based on the target's backend type:
   - **`s3` / `b2` backend**: invoke the storage backend plugin on the local agent via a `replica.download` event, passing the `remote_key` and target configuration. The plugin fetches from the external storage and writes to a staging path. This requires a storage backend plugin for the target's backend type to be installed on the local agent.
   - **`directory` backend**: if the directory is on a locally accessible filesystem (e.g. a NAS mount available on this node), open the file directly using the `remote_key` as the path. If not locally accessible, skip this replica.
3. On success, cache the fetched file locally and serve from cache.
4. If no replicas are available or all fetch attempts fail, return `EIO` to the caller.

Replica failover is best-effort. It adds resilience when the owning node is down, but does not guarantee availability — it only works when at least one replica exists on a reachable target and the local agent has the means to fetch from it. The control plane does not proxy file bytes for physical nodes.

**`export_path` containment check.** When the VFS opens a local file (Tier 1), it verifies that the resolved `export_path` is under one of the agent's configured `watch_paths` after canonicalization (resolving symlinks via `realpath`). This prevents a malicious or corrupted file document from tricking the agent into serving arbitrary files outside the watched directories. The check is: `canonical_export_path.starts_with(canonical_watch_path)` for at least one watch path. Files that fail this check are rejected with a 403. Nodes hosting source-mode storage backends skip this check — their files are served from the VFS cache or materialized by plugins, not read from arbitrary paths.
