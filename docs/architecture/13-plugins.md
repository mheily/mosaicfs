<\!-- MosaicFS Architecture · ../architecture.md -->

## Plugin System

### Plugin Runner Architecture

The plugin runner is a subsystem of the agent responsible for delivering file lifecycle events to configured plugins and writing annotation results back to CouchDB. It operates independently of the crawl and watch pipeline — the crawler and watcher enqueue events into a SQLite job table; the plugin runner drains that queue asynchronously.

```
/var/lib/mosaicfs/plugin_jobs.db   ← SQLite job queue (separate from cache index.db)

  table: jobs
    id           INTEGER PRIMARY KEY
    node_id      TEXT
    export_path  TEXT
    plugin_name  TEXT
    event_type   TEXT        -- file.added | file.modified | file.deleted | access.updated | sync.started | sync.completed | crawl_requested | materialize | replica.upload | replica.download | replica.delete | replica.list | replica.health
    payload_json TEXT        -- serialised event payload
    sequence     INTEGER     -- monotonically increasing per plugin
    status       TEXT        -- pending | in_flight | acked | failed
    attempts     INTEGER DEFAULT 0
    next_attempt TEXT        -- ISO 8601, for backoff scheduling
    created_at   TEXT
```

For executable plugins, workers poll the `pending` queue, set status to `in_flight`, invoke the binary, and on success write the annotation document and mark the job `acked`. On failure they increment `attempts` and set `next_attempt` with exponential backoff. After `max_attempts`, the job moves to `failed` and is surfaced in `agent_status`.

For socket plugins, the runner maintains one connection per plugin. Events are written to the socket in sequence order as soon as the socket is connected. When an ack arrives, the corresponding job row is updated to `acked`. On disconnect, all `in_flight` rows are reset to `pending` for replay on reconnect.

**Queue size limit.** The job queue is capped at 100,000 pending jobs per plugin. When the cap is reached, new events for that plugin are dropped and a notification is written (`notification::<node_id>::plugin_queue_full:<plugin_name>`). The notification includes guidance to either fix the plugin or trigger a full sync after the plugin is healthy again (which will re-process any files missed during the drop window). Completed (`acked`) and permanently failed jobs are purged from the queue after 24 hours to prevent unbounded database growth.

### Plugin Full Sync

A full sync replays all active files in the local CouchDB replica through the plugin pipeline. It is triggered by a manual user action from the web UI (per-plugin or for all plugins on the node) or by the `POST /api/nodes/{node_id}/plugins/{plugin_name}/sync` API endpoint.

```
full_sync(plugin_name)
  → emit sync.started event to plugin
  → query all file documents where status = "active" from local CouchDB
  → for each file:
      fetch annotation document for (file, plugin_name) if it exists
      if annotation.annotated_at >= file.mtime: skip  ← already current
      enqueue file.added job for this file
  → emit sync.completed event to plugin
```

This design means the full sync is idempotent — running it multiple times is safe. Files whose annotations are already current are skipped. Newly installed plugins have no annotation documents, so every file is enqueued. A plugin that crashed mid-sync will have a mix of annotated and unannotated files; the sync skips the ones already processed.

The `sync.started` and `sync.completed` events allow socket plugins to optimize their handling. A search indexer might suppress incremental commits during the sync window and perform one bulk commit on `sync.completed`, which is significantly faster than committing after every file.

### Available Plugins Discovery

On startup and whenever the plugin directory changes, the agent enumerates the platform-specific plugin directory and records the list of executable filenames in `agent_status.available_plugins`. The web UI reads this list when a user creates a new plugin configuration, populating the plugin name dropdown with only the binaries that are actually installed on that node. Attempting to create a plugin configuration for a name not in `available_plugins` is permitted (the binary could be installed later) but is flagged with a warning in the UI.

### Capability Advertisement

When a plugin comes online and has `query_endpoints` declared, the agent adds the corresponding capability strings to the node document's `capabilities` array. When a plugin goes offline (socket disconnects, binary missing, max_attempts exhausted), its capabilities are removed. The node document update is a standard CouchDB write that replicates to the control plane and browser within seconds.

The control plane maintains a live view of `capabilities` across all nodes by watching the CouchDB changes feed. When a `POST /api/query` request arrives, the control plane fans it out to all nodes whose `capabilities` array contains the requested capability. This means adding a new query capability to the system requires only: deploying a plugin binary with `query_endpoints` declared in its plugin document. No control plane code changes, no UI code changes.

```
capability advertisement lifecycle:

plugin socket connects
  → agent adds capability to node.capabilities
  → node document written to CouchDB
  → replicates to control plane within seconds
  → control plane begins routing queries to this node

plugin socket disconnects
  → agent removes capability from node.capabilities
  → control plane stops routing queries to this node
  → in-flight queries to this node time out gracefully
```

### Plugin Query Routing

The control plane exposes `POST /api/query` as the single query entry point for the browser. The request body specifies a `capability` and a `query` string. The control plane fans the request out to all online nodes advertising that capability, collects responses, and returns them as an array.

```
POST /api/query
  { "capability": "search", "query": "quarterly earnings" }

  → control plane finds all nodes where capabilities ∋ "search"
  → for each matching node:
      POST node.transfer.endpoint/query
           { "query": "quarterly earnings", "capability": "search" }
      collect response or record timeout
  → return array of result envelopes to browser:
  [
    {
      "node_id":     "plugin-agent-01",
      "plugin_name": "fulltext-search",
      "capability":  "search",
      "description": "Full-text search powered by Meilisearch",
      "results":     [ ... ]
    },
    {
      "node_id":     "plugin-agent-01",
      "plugin_name": "semantic-search",
      "capability":  "search",
      "description": "Semantic similarity search",
      "results":     [ ... ]
    }
  ]
```

Nodes that do not respond within a configurable timeout (default 5 seconds) are omitted from the results — a slow or unavailable plugin degrades gracefully rather than blocking the entire query. The browser renders each result envelope as a separate labelled section, identified by `description`.

**Plugin-agent node in Docker Compose**

A plugin-agent is a standard MosaicFS agent running in the Docker Compose stack alongside the control plane, with no watch paths and no VFS mount. It is indistinguishable from a regular agent in the data model. The only difference is operational: it runs in a container with access to the Compose internal network, allowing plugin binaries to reach services like Meilisearch by hostname.

```yaml
services:
  control-plane:
    image: mosaicfs-server

  meilisearch:
    image: getmeili/meilisearch
    volumes:
      - meilisearch_data:/meili_data

  plugin-agent:
    image: mosaicfs-agent
    environment:
      - MOSAICFS_SERVER_URL=http://control-plane:8080
      - MOSAICFS_SECRET_KEY=...
    volumes:
      - ./plugins:/usr/lib/mosaicfs/plugins
```

The `fulltext-search` plugin binary in `/usr/lib/mosaicfs/plugins/` connects to `meilisearch:7700` directly over the Compose network. No external networking, no additional credentials. The indexing plugin (agent side, running on each physical node) and the query plugin (running on the plugin-agent node) are two separate binaries that share the same Meilisearch instance — the indexing plugin writes, the query plugin reads.

### Storage Backend Plugins

Storage backend plugins are thin adapters that move bytes between the agent and external storage services. In target mode, they are used by the agent's replication subsystem to upload, download, and delete file replicas. In source mode, they poll external services for file metadata and materialize file content on demand. The replication subsystem handles all orchestration (rule evaluation, scheduling, bandwidth limiting, state tracking); the plugin handles only I/O with a specific storage service.

Storage backend plugins subscribe to a dedicated set of replication events:

| Event | Description |
|---|---|
| `replica.upload` | Upload a file to the target. The agent provides the local file path, remote key, and target configuration. The plugin uploads the file and returns the remote key and checksum. |
| `replica.download` | Download a file from the target. The agent provides the remote key, staging path, and target configuration. The plugin fetches the file and writes it to the staging path. Used by Tier 4b failover. |
| `replica.delete` | Delete a file from the target by remote key. |
| `replica.list` | List all objects on the target under the configured prefix. Used for manifest reconciliation and restore operations. Returns a stream of `{ remote_key, size, mtime }` entries. |
| `replica.health` | Check connectivity and credentials for the target. Returns status and any error details. |
| `source.crawl` | Source mode: poll the external service and return file operations (creates, updates, deletes). Used by source-mode backends to poll the external service. |
| `source.materialize` | Source mode: extract a specific file from the service and write it to a staging path. Used by the transfer server when a file is requested. |

**Example plugin document for a B2 storage backend:**

```json
{
  "_id": "plugin::node-laptop::backend-b2",
  "type": "plugin",
  "node_id": "node-laptop",
  "plugin_name": "backend-b2",
  "plugin_type": "socket",
  "enabled": true,
  "name": "Backblaze B2 Storage Backend",
  "subscribed_events": ["replica.upload", "replica.download", "replica.delete", "replica.list", "replica.health"],
  "mime_globs": [],
  "config": {},
  "workers": 4,
  "timeout_s": 300,
  "max_attempts": 3
}
```

**Upload event envelope:**

```json
{
  "event": "replica.upload",
  "payload": {
    "file_id": "file::a3f9...",
    "source_path": "/var/lib/mosaicfs/cache/a3/f72b1c...",
    "remote_key": "mosaicfs/node-laptop/a3f92b1c/report.pdf",
    "target_config": { "bucket": "my-backups", "prefix": "mosaicfs/" },
    "credentials": { "app_key_id": "...", "app_key": "..." }
  }
}
```

**Upload response:**

```json
{
  "status": "ok",
  "remote_key": "mosaicfs/node-laptop/a3f92b1c/report.pdf",
  "checksum": "sha256:abc123...",
  "bytes_uploaded": 204800
}
```

The agent resolves credentials from the replication target's `credentials_ref` before invoking the plugin — the plugin never accesses CouchDB directly.

Socket plugins are preferred for storage backends because they maintain authenticated sessions and connection pools across invocations, amortizing OAuth token refresh and TLS handshake costs. The `agent` backend type does not require a plugin — the agent handles agent-to-agent transfer natively using the existing transfer endpoint.

---

### Source-Mode Storage Backends

A source-mode storage backend extends the agent to index files from external data sources — cloud services, email providers, calendar APIs. The agent hosting a source-mode backend may have no real filesystem watch paths and instead relies on storage backend plugins to provide its filesystem implementation. This extends the plugin model to cover not just file processing and annotation, but file *creation* from external data sources.

**Relationship to the existing agent model**

An agent hosting source-mode storage backends runs the standard `mosaicfs-agent` binary. It participates in the same document model, replication flows, and health monitoring as any other agent. The only operational differences are:

- `agent.toml` may declare no `watch_paths` if the agent exists solely to host storage backends
- One or more plugins have `provides_filesystem: true` and a unique `file_path_prefix`
- The node has dedicated storage (a Docker volume) for backend data
- The agent invokes `source.crawl` events on its storage backend plugins instead of walking directories

**Backend storage**

Backend storage is a Docker volume mounted into the agent container at a well-known path. It is divided into two directories:

```
/var/lib/mosaicfs/backend-data/
  files/              ← the export tree — paths appearing in file document export_paths
    gmail/
      2026/02/
        re-project-kickoff.eml
  plugin-state/       ← plugin-managed internal state, not exposed as MosaicFS files
    email-fetch/
      messages.db     ← SQLite: raw message storage for Option 2 aggregate storage
      sync-state.json ← last sync cursor, OAuth tokens, etc.
    fulltext-search/
      index.db        ← search index maintained by the indexing plugin
```

`files/` contains real files on disk — the content that appears in the virtual filesystem. Plugins write to `files/` during crawl and delete from `files/` when data is retired. The agent serves these files directly via Tier 1 (local file) since the hosting agent owns them. No Tier 5 materialize invocation is needed for files that exist on disk in `files/`.

`plugin-state/` is opaque to MosaicFS. Plugins organize it however they need. It is included in full backups (it is part of the Docker volume) but excluded from minimal backups (it is regenerable by re-running the plugin).

**Two storage strategies for plugin-owned files**

Source-mode backend plugins can choose how they store data internally:

*Option A — One file per record (files/ approach).* The plugin writes one `.eml` per email, one `.ics` per calendar event, organized under `files/` with date-based sharding to avoid directory size problems. The agent serves them via Tier 1. No materialize invocation needed. Simple, debuggable, compatible with standard file tools.

```
files/gmail/2026/02/16/re-project-kickoff.eml   ← real file on disk
files/gmail/2026/02/16/status-update.eml
```

Volume format: `mkfs.ext4 -N 2000000` to provision sufficient inodes for large email archives. The agent monitors inode utilization on backend storage and writes an `inodes_near_exhaustion` notification when approaching the limit.

*Option B — Aggregate storage with Tier 5 materialize.* The plugin stores all records in `plugin-state/` (e.g., a SQLite database of message bodies) and registers files in CouchDB with export paths under `files/`. Files do not exist on disk until first access, at which point the transfer server invokes the `materialize` action and the agent writes the result into the VFS cache. Subsequent accesses hit the cache.

```
plugin-state/email-fetch/messages.db   ← SQLite with all message bodies
                                          (no individual .eml files on disk)
```

Option A is recommended for v1. Option B is the upgrade path if inode exhaustion becomes a problem in practice — the `provides_filesystem` and `file_path_prefix` fields are already in the schema, and the Tier 5 materialize path is already implemented, so the switch requires only a plugin implementation change with no agent or schema updates.

**Source-mode backend agent in Docker Compose**

```yaml
services:
  control-plane:
    image: mosaicfs-server

  agent-email:
    image: mosaicfs-agent
    environment:
      - MOSAICFS_SERVER_URL=http://control-plane:8080
      - MOSAICFS_SECRET_KEY=...
      - MOSAICFS_NODE_NAME=Email Agent
    volumes:
      - ./plugins:/usr/lib/mosaicfs/plugins
      - email-backend-data:/var/lib/mosaicfs/backend-data

volumes:
  email-backend-data:
    driver: local
    driver_opts:
      type: none
      o: bind
      device: /mnt/data/mosaicfs-email-backend
```

The `agent.toml` for an agent hosting source-mode backends:

```toml
[agent]
watch_paths = []   # no real filesystem to watch

[storage_backends]
backend_data_path = "/var/lib/mosaicfs/backend-data"
```

**Node document for an agent hosting source-mode backends**

The node document for an agent hosting source-mode backends is identical to any other agent. The presence of `storage_backend` documents with `hosting_node_id` pointing to this node is what distinguishes it from a regular agent.

```json
{
  "_id":           "node::agent-email-01",
  "type":          "node",
  "friendly_name": "Email Agent",
  "platform":      "linux",
  "vfs_capable":   false,
  "storage": [
    {
      "filesystem_id":       "email-backend-data",
      "mount_point":         "/var/lib/mosaicfs/backend-data",
      "fs_type":             "ext4",
      "capacity_bytes":      107374182400,
      "used_bytes":          21474836480,
      "watch_paths_on_fs":   []
    }
  ]
}
```

**Agent main loop for source-mode backend agents**

The crawl step is replaced by plugin invocation:

```
startup (source-mode backend agent)
  → load config
  → connect to local CouchDB
  → start CouchDB replication (bidirectional, continuous)
  → load plugin configurations → find plugins where provides_filesystem = true
  → for each filesystem-providing plugin:
      emit crawl_requested (trigger: "startup") → apply file operations to CouchDB
  → start transfer HTTP server  (serves files from backend-data/files/ via Tier 1)
  → start heartbeat
  → start plugin runner (for non-filesystem plugins: annotators, indexers, etc.)
  → on schedule (nightly): emit crawl_requested (trigger: "scheduled") to each filesystem plugin
  → on schedule (hourly):  collect backend storage utilization → write utilization_snapshot
                           check inode utilization → write/resolve inodes_near_exhaustion
                           check storage utilization → write/resolve storage_near_capacity
  → on manual sync request: emit crawl_requested (trigger: "manual")
```

**Backend storage monitoring**

The agent's hourly utilization check covers both disk space and inode utilization on backend storage volumes. Two additional notification condition keys are defined for agents hosting source-mode backends:

| Condition Key | Severity | Auto-resolves |
|---|---|---|
| `inodes_near_exhaustion` | warning | Yes (clears when inodes freed) |
| `storage_near_capacity` | warning | Yes (clears when space freed) |

**Plugin document example for an email source-mode backend plugin**

```json
{
  "_id":                "plugin::agent-email-01::email-fetch",
  "type":               "plugin",
  "node_id":            "agent-email-01",
  "plugin_name":        "email-fetch",
  "plugin_type":        "executable",
  "enabled":            true,
  "name":               "Gmail Fetcher",
  "subscribed_events":  ["crawl_requested"],
  "provides_filesystem": true,
  "file_path_prefix":   "/gmail",
  "settings_schema": {
    "properties": {
      "gmail_client_id":     { "type": "string", "title": "Gmail Client ID" },
      "gmail_client_secret": { "type": "secret", "title": "Gmail Client Secret" },
      "fetch_all":           { "type": "boolean", "title": "Fetch all mail", "default": true },
      "fetch_days":          { "type": "number",  "title": "Fetch last N days (if not all)", "default": 30 },
      "auto_delete_days":    { "type": "number",  "title": "Auto-delete older than N days (0 = never)", "default": 0 }
    },
    "required": ["gmail_client_id", "gmail_client_secret"]
  },
  "settings": {
    "gmail_client_id": "...",
    "gmail_client_secret": "...",
    "fetch_all": true,
    "auto_delete_days": 0
  },
  "created_at": "2026-02-16T09:00:00Z"
}
```

Other plugins on the same agent (annotators, search indexers) are configured identically to plugins on physical nodes — they subscribe to `file.added`, `file.modified`, and `file.deleted` events that the agent emits as the email-fetch plugin's crawl responses are applied to CouchDB.

**Privilege model.** Executable plugins run with the same user and group as the agent process. On Linux, this is typically `root` because FUSE mounting requires elevated privileges. This means a plugin binary has full system access. The security boundary is the plugin directory — only binaries placed there by a local administrator can be executed. v1 does not implement additional sandboxing (seccomp, namespaces, capability drops). If fine-grained plugin isolation is needed in a future version, the agent could spawn plugins inside a restricted namespace or use a dedicated unprivileged user with bind-mounted access to the cache and plugin state directories.

The plugin directory is the security boundary. An entry in a `plugin` CouchDB document cannot cause the agent to execute a binary outside the plugin directory, regardless of what `plugin_name` contains. The agent resolves `plugin_name` by joining it to the plugin directory path and checking that the resulting path is a regular, executable file — path traversal characters (`.`, `/`) in `plugin_name` are rejected as a permanent error. A malicious `plugin_name` value of `../../bin/sh` fails immediately rather than executing a shell.

The `config` object in the plugin document is passed to the plugin on stdin as part of the event envelope. It is treated as data, not as shell arguments — there is no shell interpolation at any point in the invocation path. The agent invokes the binary directly via `execv`, not via a shell.

### Future Directions

The Unix socket model extends naturally to networked plugins: replacing the socket path with a TCP address would allow plugins to run on a different machine from the agent. The event envelope and ack protocol are identical. This is not implemented in v1 but the document schema and protocol are designed to accommodate it — a future `plugin_type: "tcp"` variant would add a `socket_address` field alongside the existing `plugin_name`.

---

