<\!-- MosaicFS Architecture · ../architecture.md -->

## VFS Tiered Access Strategy

When the VFS layer needs to open a file, it evaluates access tiers in order of increasing cost, stopping at the first available option. This logic lives in the common `mosaicfs-vfs` crate and is shared across all OS-specific backends:

All tiers record a file access via the access tracker (`access_tracker.record(file_id)`) — an in-memory update with no I/O on the hot path. See the Access Tracking Document section for details on debounced persistence.

**Tier 1 — Local file.** The file lives on this node. Open directly via the real path.

**Tier 2 — Network mount (CIFS/NFS).** The owning node's document contains a `network_mounts` entry covering this file's export path. Translate and open via the local mount point recorded in that entry.

**Tier 3 — Local cloud sync directory.** The owning node's document contains a `network_mounts` entry of type `icloud_local` or `gdrive_local` covering this file. Open via the local sync directory, with eviction check for iCloud. If the file is evicted from local iCloud storage, fall through to Tier 4 rather than triggering an implicit cloud download.

**Tier 4 — Remote fetch.** No local access path is available. The requesting agent fetches the file from the owning agent's transfer server, caches it locally, and serves from cache. The discovery sequence is:

1. Look up the file document in the local CouchDB replica to get `source.node_id`.
2. Look up `node::<source.node_id>` in the local replica to get `transfer.endpoint` (host:port) and `status`.
3. If the owning node is `online`, send `GET http://<transfer.endpoint>/api/agent/transfer/{file_id}` with HMAC-signed request headers.
4. If the owning node is `offline` or the transfer request fails with a connection error, fall through to Tier 4b.
5. Stream the response to `cache/tmp/{cache_key}`, verify the `Digest` trailer (SHA-256), atomic-rename into `cache/{shard}/{cache_key}`, and serve from cache.

**Tier 4b — Replica failover.** Evaluated when Tier 4 fails because the owning node is offline or unreachable. The VFS checks for available replicas of the file and attempts to fetch from one:

1. Query the local CouchDB replica for `replica` documents matching this file's `file_id` with `status` of `"current"` or `"frozen"`.
2. For each available replica, attempt to fetch based on the target's backend type:
   - **`agent` backend**: send `GET /api/agent/transfer/{file_id}` to the replica agent (same as Tier 4, but targeting a different node). The replica agent serves the file from its replication storage path (configured via `transfer_serve_paths` in `agent.toml`).
   - **`s3` / `b2` backend**: invoke the storage backend plugin on the local agent via a `replica.download` event, passing the `remote_key` and target configuration. The plugin fetches from the external storage and writes to a staging path. This requires a storage backend plugin for the target's backend type to be installed on the local agent.
   - **`directory` backend**: if the directory is on a locally accessible filesystem (e.g. a NAS mount available on this node), open the file directly using the `remote_key` as the path. If not locally accessible, skip this replica.
3. On success, cache the fetched file locally (same as Tier 4) and serve from cache.
4. If no replicas are available or all fetch attempts fail, return `EIO` to the caller.

Tier 4b is best-effort. It adds resilience when the owning node is down, but does not guarantee availability — it only works when at least one replica exists on a reachable target and the local agent has the means to fetch from it. The control plane does not proxy file bytes for physical nodes.

For agents hosting source-mode storage backends, the transfer endpoint is that agent's own transfer server.

**Tier 5 — Plugin materialize.** The file's `export_path` matches the `file_path_prefix` of a `provides_filesystem` plugin on the owning node. The transfer server on the owning node invokes the plugin's `materialize` action, which writes the file to `cache/tmp/`. The agent moves it into the VFS cache and serves from there. Subsequent requests hit the cache directly without involving the plugin. Tier 5 is not evaluated by the requesting agent — it is triggered on the owning agent when a Tier 4 transfer request arrives for a file that requires materialization.

**`export_path` containment check.** When the transfer server opens a local file (Tier 1), it verifies that the resolved `export_path` is under one of the agent's configured `watch_paths` after canonicalization (resolving symlinks via `realpath`). This prevents a malicious or corrupted file document from tricking the agent into serving arbitrary files outside the watched directories. The check is: `canonical_export_path.starts_with(canonical_watch_path)` for at least one watch path. Files that fail this check are rejected with a 403. Nodes hosting source-mode storage backends skip this check — their files are served from the VFS cache or materialized by plugins, not read from arbitrary paths.

The full transfer server logic on the owning agent:

```
GET /api/agent/transfer/{file_id}
  → look up file document → get file_uuid, node_id, export_path, mtime, size
  → compute cache_key = file_uuid
  → check cache/index.db:
      hit and mtime/size matches → serve from cache/{shard}/{cache_key}  ← fast path
      miss or stale:
        → check if export_path matches any plugin's file_path_prefix
        → if yes (Tier 5):
            staging_path = cache/tmp/plugin-{file_id}
            invoke plugin: materialize event with file_id, export_path, staging_path
            plugin writes bytes to staging_path, returns { size }
            move staging_path → cache/{shard}/{cache_key}  (atomic)
            insert/update row in cache/index.db
            serve from cache/{shard}/{cache_key}
        → if no (Tier 4 — remote file on another node):
            fetch via GET /api/agent/transfer/{file_id} from owning node
            stream to cache/tmp/{cache_key}
            verify Digest trailer
            move to cache/{shard}/{cache_key}
            insert row in cache/index.db
            serve from cache
```

The Digest trailer step is skipped for Tier 5 materialize — the plugin writes locally and there is no network transfer to verify. TLS is not involved. The agent trusts the plugin's output for the same reason it trusts any local process writing to the cache directory.

---

