<\!-- MosaicFS Architecture · ../architecture.md -->

## REST API Reference

The control plane exposes a single REST API consumed by all clients — the web UI, CLI, desktop app, and agents. All endpoints are prefixed with `/api/`. Client-facing endpoints (web UI, CLI, desktop app) authenticate with a Bearer JWT obtained from `POST /api/auth/login`. Agent-internal endpoints under `/api/agent/` authenticate with HMAC request signing.

**Response conventions:**
- List responses: `{ "items": [...], "total": n, "offset": n, "limit": n }`
- Single resource responses: the document object directly
- Errors: `{ "error": { "code": "...", "message": "..." } }`
- Pagination: `?limit=` (default 100, max 500) and `?offset=` on all list endpoints
- No CouchDB internals (`_rev`, `_id` prefixes, CouchDB error codes) are exposed through the API
- **Versioning:** The v1 API is unversioned — all endpoints live under `/api/`. If a breaking change is needed in a future version, a `/api/v2/` prefix will be introduced and the original `/api/` endpoints will continue to work as aliases for v1 for at least one release cycle. Additive changes (new fields in responses, new optional query parameters, new endpoints) are not considered breaking and may be added at any time. Clients should ignore unknown fields in responses.

### Auth

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/auth/login` | Exchange access key credentials for a JWT. Request body: `{ access_key_id, secret_key }`. Returns `{ token, expires_at }`. Rate-limited to 5 attempts per minute per source IP. Failed attempts return a generic 401 regardless of whether the access key ID exists, to prevent credential enumeration. |
| `POST` | `/api/auth/logout` | Invalidate the current JWT. |
| `GET` | `/api/auth/whoami` | Return the current credential's name, type, and last-seen timestamp. |

### Nodes

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/nodes` | List all nodes. Supports `?status=online\|offline\|degraded` filter. |
| `GET` | `/api/nodes/{node_id}` | Get full node document including embedded `network_mounts`. |
| `POST` | `/api/nodes` | Register a new node. Called by `mosaicfs-agent init`. Returns the new node ID. |
| `PATCH` | `/api/nodes/{node_id}` | Update `friendly_name` or `watch_paths`. |
| `DELETE` | `/api/nodes/{node_id}` | Deregister a node. Sets status to `"disabled"`; does not delete the document. |
| `GET` | `/api/nodes/{node_id}/status` | Return the node's current `agent_status` document. |
| `GET` | `/api/nodes/{node_id}/files` | List files owned by this node. Paginated. |
| `GET` | `/api/nodes/{node_id}/storage` | Return storage topology and latest utilization snapshot for this node. |
| `GET` | `/api/nodes/{node_id}/utilization` | Return utilization snapshot history. Supports `?days=30`. |
| `POST` | `/api/nodes/{node_id}/sync` | Trigger an immediate sync for nodes hosting source-mode storage backends. Returns `405` for nodes without source-mode backends. |
| `GET` | `/api/nodes/{node_id}/auth` | Return OAuth status for storage backends on this node. Returns `405` if no backends require OAuth. |
| `DELETE` | `/api/nodes/{node_id}/auth` | Revoke stored OAuth tokens for storage backends on this node. |
| `POST` | `/api/nodes/{node_id}/auth/callback` | OAuth redirect target. Receives the authorization code and exchanges it for tokens. |

### Node Network Mounts

Mounts are embedded in the node document. These endpoints update the `network_mounts` array on the node document via the control plane API.

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/nodes/{node_id}/mounts` | List all network mounts declared on this node. |
| `POST` | `/api/nodes/{node_id}/mounts` | Add a network mount. Request body: `{ remote_node_id, remote_base_export_path, local_mount_path, mount_type, priority }`. |
| `PATCH` | `/api/nodes/{node_id}/mounts/{mount_id}` | Update a mount entry. |
| `DELETE` | `/api/nodes/{node_id}/mounts/{mount_id}` | Remove a mount entry. |

### Files

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/files` | List files. Supports `?node_id=`, `?status=active\|deleted`, `?mime_type=`. Paginated. |
| `GET` | `/api/files/{file_id}` | Get file metadata document. |
| `GET` | `/api/files/{file_id}/content` | Download file bytes. Supports `Range` headers for partial content. Sets `Content-Disposition` for browser downloads. Full-file responses (HTTP 200) include a `Digest` trailer (RFC 9530, `sha-256`) computed as the bytes stream — clients may verify after receipt. Range responses (HTTP 206) do not include a `Digest` trailer. The control plane resolves the owning node and proxies bytes transparently — the client does not need to know which node holds the file. Records a file access via the access tracker. |
| `GET` | `/api/files/{file_id}/access` | Get the access tracking document for a file. Returns `last_access` and `access_count`. Returns `404` if the file has never been accessed through MosaicFS. |
| `GET` | `/api/files/by-path?path=...` | Resolve a virtual path to its file document. Returns `404` if no file is mapped to that path. |

### Virtual Filesystem

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/vfs?path=...` | List the contents of a virtual directory. Evaluates the directory's mount sources on demand and returns matching files and subdirectories. |
| `GET` | `/api/vfs/tree?path=...&depth=n` | Recursive directory tree from a given path, up to `depth` levels deep (default 3, max 10). |
| `POST` | `/api/vfs/directories` | Create a new virtual directory. Request body: `{ virtual_path, name }`. Returns the created directory document. The directory is initially empty — add mount sources via `PATCH`. |
| `GET` | `/api/vfs/directories/{path}` | Get a virtual directory document including its full `mounts` array. |
| `PATCH` | `/api/vfs/directories/{path}` | Update a directory: rename, toggle `enforce_steps_on_children`, add/replace/remove mount entries. |
| `DELETE` | `/api/vfs/directories/{path}` | Delete a virtual directory. Returns `409` if the directory has children. Pass `?force=true` to cascade-delete all descendants. Cascade deletion removes all descendant virtual directory documents. `system: true` directories cannot be deleted. |
| `POST` | `/api/vfs/directories/{path}/preview` | Evaluate the directory's current mount sources against the file index and return matching files. The request body may contain a draft `mounts` array not yet saved — preview runs against the submitted configuration. Paginated. |

### Search

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/search?q=...` | Search files by filename. Supports substring and glob patterns. Returns `name`, `source.node_id`, `size`, `mtime` per result. Supports `?limit=` and `?offset=`. |
| `GET` | `/api/search?label=...` | Filter files by label. Returns all active files whose effective label set (direct assignments ∪ matching label rules) contains the specified label. Multiple `?label=` parameters are ANDed. Combinable with `?q=` for label + filename search. |
| `GET` | `/api/search?annotation[plugin/key]=value` | Filter files by annotation value. `plugin` is the plugin name and `key` is a dot-notation path into the annotation `data` object. `value` is an exact string match; prefix with `~` for regex match. Multiple `?annotation[...]` parameters are ANDed. Combinable with `?q=` and `?label=`. |
| `GET` | `/api/search?replicated=...` | Filter files by replication status. Value is a `target_name`. Returns all active files that have a `replica` document for the named target with `status: "current"`. Prefix with `!` to negate (files NOT replicated to the target). Combinable with all other search parameters. |

### Replication

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/storage-backends` | List all storage backends. |
| `POST` | `/api/storage-backends` | Create a storage backend. Request body: `{ name, backend, mode, hosting_node_id, backend_config, credentials_ref, schedule, poll_interval_s, bandwidth_limit_mbps, retention, remove_unmatched }`. |
| `GET` | `/api/storage-backends/{name}` | Get a storage backend document. Credentials are not included in the response. |
| `PATCH` | `/api/storage-backends/{name}` | Update a storage backend: change schedule, bandwidth, retention, mode, enable/disable. |
| `DELETE` | `/api/storage-backends/{name}` | Delete a storage backend. Returns `409` if any replication rules reference this backend. Pass `?force=true` to cascade-delete rules and replica documents. |
| `GET` | `/api/replication/rules` | List all replication rules. Supports `?target_name=` and `?enabled=` filters. |
| `POST` | `/api/replication/rules` | Create a replication rule. Request body: `{ name, target_name, source, steps, default_result, enabled }`. Returns the created rule document with assigned UUID. |
| `GET` | `/api/replication/rules/{rule_id}` | Get a replication rule document. |
| `PATCH` | `/api/replication/rules/{rule_id}` | Update a replication rule: change steps, source, enable/disable. |
| `DELETE` | `/api/replication/rules/{rule_id}` | Delete a replication rule. Existing replicas created by this rule are not affected. |
| `GET` | `/api/replication/replicas` | List replica documents. Supports `?file_id=`, `?target_name=`, `?status=` filters. Paginated. |
| `GET` | `/api/replication/replicas/{file_id}` | Get all replica documents for a specific file. |
| `GET` | `/api/replication/status` | Replication system overview: per-target statistics (file count, total size, pending count, last sync time). |
| `POST` | `/api/replication/sync` | Trigger a full re-evaluation of all replication rules. Enqueues replication work for files that match rules but have no current replica, and marks replicas as frozen for files that no longer match any rule. |
| `POST` | `/api/replication/restore` | Initiate a restore operation. Request body: `{ target_name, source_node_id, destination_node_id, destination_path, filters }`. `filters` is optional: `{ path_prefix, mime_type }`. Returns a job ID. |
| `GET` | `/api/replication/restore/{job_id}` | Check restore progress: files scanned, downloaded, created, errors. |
| `POST` | `/api/replication/restore/{job_id}/cancel` | Cancel an in-progress restore. Files already restored remain. |

### Migration

File migration permanently moves file ownership from one agent to another. Because file identity is location-independent (`file::{uuid}`), migration updates `source.node_id` and `source.export_path` on the existing file document rather than creating a new one. All references (label assignments, annotations, replicas, access documents) survive unchanged.

**Migration process:**

1. The control plane validates that both source and destination agents are online.
2. The destination agent downloads the file content from the source agent via `GET /api/agent/transfer/{file_id}`.
3. The destination agent writes the file to its local filesystem and confirms receipt.
4. The control plane updates the file document: `source.node_id` → destination, `source.export_path` → new path, and sets `migrated_from` with the previous owner's details.
5. The source agent deletes its local copy of the file on the next crawl cycle (it no longer matches `source.node_id`). The crawler treats it as a foreign file and ignores it; a background cleanup task removes the physical file.

**Safety checks:** The source file is not deleted until the destination confirms a successful write and the file document has been updated. If any step fails, the migration is aborted and the file remains at its original location.

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/migration` | Initiate a migration. Request body: `{ file_ids, destination_node_id, destination_path_prefix }`. `file_ids` is an array of file document `_id` values. Returns a job ID. |
| `GET` | `/api/migration/{job_id}` | Check migration progress: files transferred, pending, errors. |
| `POST` | `/api/migration/{job_id}/cancel` | Cancel an in-progress migration. Files already migrated remain at the destination; files not yet transferred remain at the source. |
| `POST` | `/api/migration/evacuate` | Evacuate all files from a node. Request body: `{ source_node_id, destination_node_id, destination_path_prefix }`. Equivalent to migrating all active files from the source. Returns a job ID. |

### Plugins

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/nodes/{node_id}/plugins` | List all plugin configurations for a node. |
| `POST` | `/api/nodes/{node_id}/plugins` | Create a plugin configuration. Request body: `{ plugin_name, plugin_type, enabled, name, subscribed_events, mime_globs, config, workers, timeout_s, max_attempts }`. |
| `GET` | `/api/nodes/{node_id}/plugins/{plugin_name}` | Get a plugin configuration document. |
| `PATCH` | `/api/nodes/{node_id}/plugins/{plugin_name}` | Update a plugin configuration. Changes take effect on the agent within one changes-feed poll cycle — no restart required. |
| `DELETE` | `/api/nodes/{node_id}/plugins/{plugin_name}` | Delete a plugin configuration. In-flight jobs complete; no new jobs are enqueued. |
| `POST` | `/api/nodes/{node_id}/plugins/{plugin_name}/sync` | Trigger a full sync for a specific plugin on a node. Enqueues `file.added` events for all active files whose annotation is stale or absent, bracketed by `sync.started` and `sync.completed`. |
| `POST` | `/api/nodes/{node_id}/sync` | Trigger a full sync for all enabled plugins on a node. |
| `GET` | `/api/nodes/{node_id}/plugins/{plugin_name}/jobs` | List recent plugin jobs — pending, in-flight, and failed. Supports `?status=` filter. Useful for diagnosing plugin failures from the web UI. |

### Query

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/query` | Fan out a query to all online nodes advertising the requested capability. Request body: `{ capability, query }`. Returns an array of result envelopes, one per responding plugin. Nodes that do not respond within the timeout are omitted. The browser does not need to know which nodes or plugins exist — it sends a capability name and receives labelled results. |

### Notifications

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/notifications` | List notifications. Supports `?status=active`, `?status=acknowledged`, `?status=resolved`, `?severity=error` filters. Default returns all active and unacknowledged notifications, sorted by severity then `last_seen`. |
| `POST` | `/api/notifications/{notification_id}/acknowledge` | Acknowledge a notification. Sets `status` to `"acknowledged"` and records `acknowledged_at`. |
| `POST` | `/api/notifications/acknowledge-all` | Acknowledge all currently active notifications. Accepts optional `?severity=` filter to acknowledge only notifications of a given severity. |
| `GET` | `/api/notifications/history` | Returns resolved and acknowledged notifications. Supports `?limit=` and `?since=` (ISO 8601). Useful for the notification history log view. |

### Annotations

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/annotations?file_id=...` | Get all annotation documents for a specific file (all plugins). Returns an object keyed by `plugin_name`. |
| `GET` | `/api/annotations?file_id=...&plugin=...` | Get the annotation document for a specific file and plugin. Returns `404` if no annotation exists. |
| `DELETE` | `/api/annotations?file_id=...` | Delete all annotation documents for a file. The next plugin sync or file event will cause the plugin to re-annotate. |
| `DELETE` | `/api/annotations?file_id=...&plugin=...` | Delete the annotation document for a specific file and plugin. |

### Labels

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/labels` | List all distinct label strings currently in use across all assignments and rules. Useful for autocomplete in the web UI. |
| `GET` | `/api/labels/assignments?file_id=...` | Get the `label_assignment` document for a specific file. Returns `404` if no labels are assigned. |
| `PUT` | `/api/labels/assignments` | Create or replace a label assignment. Request body: `{ file_id, labels }`. Overwrites any existing assignment for that file. |
| `DELETE` | `/api/labels/assignments?file_id=...` | Remove all labels from a specific file. |
| `GET` | `/api/labels/rules` | List all label rules. Supports `?node_id=` and `?enabled=` filters. |
| `POST` | `/api/labels/rules` | Create a label rule. Request body: `{ node_id, path_prefix, labels, name, enabled }`. |
| `PATCH` | `/api/labels/rules/{rule_id}` | Update a label rule: change labels, rename, enable/disable. |
| `DELETE` | `/api/labels/rules/{rule_id}` | Delete a label rule. Does not affect individual file assignments. |
| `GET` | `/api/labels/effective?file_id=...` | Compute and return the effective label set for a specific file — the union of its direct assignment and all matching rules. Useful for debugging label configuration. |

### Credentials

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/credentials` | List all credentials. Secret key hashes are never returned. |
| `GET` | `/api/credentials/{key_id}` | Get credential detail: name, type, enabled status, created_at, last_seen. |
| `POST` | `/api/credentials` | Create a new credential. Returns the secret key once in the response — it is not stored in recoverable form and cannot be retrieved again. |
| `PATCH` | `/api/credentials/{key_id}` | Update name or enabled status. |
| `DELETE` | `/api/credentials/{key_id}` | Revoke a credential. The agent or client using it will be rejected on its next request. |

### Storage

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/storage` | System-wide storage overview: capacity and utilization for all nodes and storage backends in a single response. |
| `GET` | `/api/storage/{node_id}/history` | Utilization snapshot history for a specific node. Supports `?days=30`. |

### System

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/health` | Overall system health: `healthy`, `degraded`, or `unhealthy`, with a summary of node statuses. |
| `GET` | `/api/health/nodes` | Per-node health summary for all nodes. |
| `GET` | `/api/system/info` | Instance metadata: version, setup date, total node count, total indexed file count. |
| `POST` | `/api/system/reindex` | Trigger a full reindex of all nodes. Sends a reindex command to all online agents. |
| `GET` | `/api/system/backup?type=minimal\|full` | Generate and download a backup. `minimal` includes only essential user-generated data (virtual directories, labels, annotations, credentials, plugin configurations). `full` includes the entire CouchDB database. Returns a JSON file in CouchDB `_bulk_docs` format, streamed as `Content-Disposition: attachment`. Filename: `mosaicfs-backup-{type}-{timestamp}.json`. |
| `POST` | `/api/system/restore` | Restore from a backup file. Requires an empty database — the endpoint checks the document count and rejects the restore if any documents exist. Request body is the JSON backup file. Validates that all documents have recognized `type` fields. For minimal backups, the restore process writes documents directly and merges `network_mounts` into existing node documents. For full backups, performs a bulk write of all documents. Returns a summary: document count restored, errors encountered. |
| `GET` | `/api/system/backup/status` | Check whether the database is empty (restorable). Returns `{ empty: true\|false, document_count: N }`. Used by the web UI to conditionally show the restore button. |
| `DELETE` | `/api/system/data` | **Developer mode only.** Delete all documents from CouchDB to enable restore into a non-empty database. Returns 403 Forbidden unless the control plane was started with `--developer-mode`. Requires a confirmation token in the request body: `{ "confirm": "DELETE_ALL_DATA" }`. Returns `{ deleted_count: N }` on success. This endpoint is intended for development and testing workflows where quickly cycling between backup/restore states is useful. It should never be enabled in production — the safer path is to destroy and recreate the Docker Compose stack. |

### Agent Internal

These endpoints are authenticated with HMAC request signing rather than JWT. They are not intended for use by human-operated clients. The `/api/agent/` prefix is not documented in the web UI or CLI help output.

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/agent/heartbeat` | Agent posts an updated node document (status, last_heartbeat). |
| `POST` | `/api/agent/files/bulk` | Bulk upsert file documents. Request body: `{ docs: [...] }`. Response includes per-document success or error status. Partial failure is accepted — successfully processed documents are committed even if others fail. |
| `POST` | `/api/agent/status` | Post an `agent_status` document. |
| `POST` | `/api/agent/utilization` | Post a `utilization_snapshot` document. |
| `GET` | `/api/agent/credentials` | Fetch the current enabled credential list for P2P transfer authentication. |
| `GET` | `/api/agent/transfer/{file_id}` | Fetch file bytes for a specific file. Used by remote agents requesting files via P2P transfer. Validates that the requesting credential is known and the file exists. Full-file responses include a `Digest` trailer (RFC 9530, `sha-256`); range responses do not. Supports `Range` headers. |
| `POST` | `/api/agent/query` | Deliver a query to this agent's plugin runner. Called by the control plane when routing a `POST /api/query` request to a node whose `capabilities` array contains the requested capability. Request body: `{ capability, query }`. The agent fans the request to all locally configured plugins with a matching `query_endpoints[].capability`, collects their stdout responses, and returns the merged array of result envelopes. Authenticated with HMAC — not reachable by the browser directly. |

---

