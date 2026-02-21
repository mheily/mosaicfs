<\!-- MosaicFS Architecture · ../architecture.md -->

## Open Questions

This section captures design decisions that were discussed during the design phase. Each question has been resolved with a decision for v1 implementation.

### Virtual Directory and Label Event Hooks for Plugins

**Context:** Plugins currently subscribe to file lifecycle events (`file.added`, `file.modified`, `file.deleted`). During design, we considered additional event types: `vfs.directory.created`, `vfs.directory.modified`, `vfs.directory.deleted`, and `label.assigned`, `label.removed`, `label.rule.created`, `label.rule.deleted`.

**Question:** Should these be added to v1, or held for later? 

**Arguments for deferral:** No concrete plugin use case exists yet. Virtual directory changes are user-driven and infrequent — a plugin reacting to them is more like a webhook than an annotation pipeline. Label hooks feel like workflow automation ("when labelled X, do Y") which is a different category of extension.

**Arguments for inclusion:** A hypothetical "sync to external DMS when labelled `archive`" plugin is genuinely useful. A plugin maintaining an external catalog mirroring the virtual tree structure could use directory events.

**Decision:** Deferred to v2. No schema or implementation changes needed for v1. Adding new event types is a backwards-compatible change — existing plugins simply won't subscribe to them.

---

### Plugin Query Result Streaming

**Context:** The `POST /api/query` endpoint fans out to all nodes advertising a capability, collects responses, and returns them as an array. Currently specified as gather-then-return.

**Question:** Should query results stream back to the browser as nodes respond, or wait until all nodes have responded (or timed out)?

**Tradeoffs:**
- Gather-then-return: Simple, predictable, works for v1 where plugin-agents are co-located with the control plane on fast local network
- Streaming (server-sent events or chunked JSON): Faster perceived latency, more complex to implement, better for geographically distributed nodes

**Decision:** v1 uses gather-then-return. At home-deployment scale with plugin-agents co-located on the control plane's Docker network, the latency difference is negligible. Streaming can be added as a backwards-compatible enhancement if geographically distributed nodes become a use case.

---

### Source-Mode Backend Storage: Option A vs Option B Threshold

**Context:** Source-mode storage backends can use Option A (one file per record, direct Tier 1 serving) or Option B (aggregate SQLite storage with Tier 5 materialize on demand).

**Question:** At what point should a deployment switch from Option A to Option B? Is there a file count threshold, or should it always be the plugin author's choice?

**Considerations:**
- Inode exhaustion on ext4 starts becoming a concern around 500K-1M small files
- Date-based sharding helps but doesn't eliminate the problem
- Option B adds materialize latency on first access, but VFS cache makes subsequent accesses fast
- Plugin implementation complexity: Option B requires maintaining SQLite schema and materialize logic; Option A is just "write .eml files"

**Decision:** Option A is the default for v1. Plugin authors choose based on their data characteristics. Guidance: use Option A for data sources producing fewer than 500K files with an average size above 1 KB (email, documents, photos). Use Option B for data sources producing millions of tiny records or where on-demand extraction from an API is natural (chat messages, calendar events, social media). The `mkfs.ext4 -N 2000000` recommendation for backend storage volumes remains as a practical mitigation for Option A deployments approaching inode limits.

---

### Scheduled Automatic Backups

**Context:** Backup and restore are on-demand in v1 via the REST API and Settings page. User downloads the JSON file and stores it wherever they want.

**Question:** Should MosaicFS support scheduled automatic backups to a configured destination (S3 bucket, local directory, NAS share)?

**Considerations:**
- Natural extension of the backup feature
- Requires destination configuration (credentials, paths)
- Rotation policy (keep last N backups, delete older than X days)
- Notification on backup failure

**Decision:** Deferred to v2. The backup API and format are stable — automated scheduling is a pure addition with no impact on existing backup/restore functionality. Users who need automated backups in v1 can script `curl` against the backup endpoint.

---

### Global Settings Scope for Source-Mode Plugins

**Context:** Source-mode plugins (email-fetch, caldav-sync) might be deployed on multiple nodes. Settings like "Gmail API endpoint URL" or "Meilisearch connection string" are likely the same across all instances of a plugin.

**Question:** Should plugin settings support a `settings_scope` field distinguishing between `"node"` (per-node configuration) and `"global"` (shared across all instances)?

**Considerations:**
- Per-node is consistent with the current model but tedious if every node needs identical settings
- Global settings would be stored in a synthetic `plugin::{plugin_name}` document without a node ID
- Agent falls back to global settings when no node-specific override exists
- Adds complexity to the settings merge logic

**Decision:** Deferred to v2. Per-node settings are sufficient for v1 where source-mode plugins are typically deployed on a single node. If a user deploys the same plugin on multiple nodes, they can copy settings via the API. The `plugin::` ID scheme reserves room for a future `plugin::{plugin_name}` global settings document without conflicting with `plugin::{node_id}::{plugin_name}` per-node documents.

---

### Networked Socket Plugins (TCP)

**Context:** Socket plugins currently use Unix domain sockets. The architecture notes that "the Unix socket model extends naturally to networked plugins: replacing the socket path with a TCP address would allow plugins to run on a different machine from the agent."

**Question:** Is this a planned v2 feature, or just a noted possibility?

**Considerations:**
- Enables plugin deployment on dedicated hardware (GPU servers for AI plugins)
- Requires rethinking security — TCP sockets need authentication, Unix sockets inherit filesystem permissions
- Event delivery over TCP needs TLS

**Decision:** Deferred to v2. The event envelope and ack protocol are transport-agnostic by design. A future `plugin_type: "tcp"` variant would add `socket_address` and `tls_cert` fields to the plugin document. No v1 schema changes needed — the `plugin_type` enum is open to extension.

---

### Push-based Plugin Notifications

**Context:** Socket plugins are polled for health status on a configurable interval (default 5 minutes). Plugins can report notifications in the health check response.

**Question:** Should socket plugins be able to push notifications at arbitrary times, rather than waiting for the next health check poll?

**Tradeoffs:**
- Pull (current): Simple agent implementation, tolerates latency, plugin can't overwhelm agent
- Push (future): Lower latency for urgent notifications, requires unsolicited message handling on socket

**Decision:** v1 uses pull-based health checks. The socket protocol is designed to accept unsolicited messages in a future version — adding an inbound `{ "type": "notification" }` message handler is a small, backwards-compatible change. v1 plugins that need to surface urgent issues should write directly to the plugin's state directory and let the next health check pick it up.

---

*MosaicFS Architecture Document v0.1 — Subject to revision*
