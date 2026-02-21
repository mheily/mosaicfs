<\!-- MosaicFS Architecture · ../architecture.md -->

## Observability

### Logging

All components use the Rust `tracing` crate for structured logging with consistent key-value fields. Production deployments run at `INFO` level with runtime-adjustable log levels. Log files rotate at 50 MB, keeping 5 rotated files (250 MB total). Logs go to stderr in development and to a rolling file in production.

### Health Checks

Each agent exposes `GET /health` returning a JSON object with per-subsystem status (`pouchdb`, `replication`, `vfs_mount`, `watcher`, `transfer_server`, `plugins`). The `plugins` subsystem reports the status of each configured plugin: connected/disconnected for socket plugins, failed job counts and last-error for executable plugins. The control plane polls agents every 30 seconds. A node missing three consecutive checks (90 seconds with no successful health check) is marked `offline` in its node document, which flows through to the web UI status dashboard.

**Stale status detection.** The control plane is the sole authority on node online/offline status. Agents write `last_heartbeat` timestamps to their node documents, but the control plane sets the `status` field based on its own polling. This means an agent that crashes without a clean shutdown is detected within 90 seconds. On clean shutdown, the agent sets its own `status` to `"offline"` immediately for faster UI feedback. When the control plane itself restarts, it treats all nodes as unknown and begins polling — nodes that respond are marked `online`; nodes that don't respond within 90 seconds are marked `offline`. There is no scenario where a node is stuck in `"online"` indefinitely without being polled.

### Error Classification

Errors are classified as:

- **Transient** — expected to resolve; retried with exponential backoff.
- **Permanent** — require intervention; immediately surfaced to `ERROR` log and web UI.
- **Soft** — partial success; logged but operation continues.

**Retry parameters.** All transient retries across the agent use the same backoff algorithm: initial delay 1 second, multiplied by 2 on each attempt, capped at 60 seconds, with ±25% random jitter to prevent thundering herds. Specific retry contexts:

| Context | Max attempts | On exhaustion |
|---|---|---|
| Plugin executable job | Configured via `max_attempts` (default 3) | Job marked `failed`, surfaced in `agent_status` and as a notification |
| Socket plugin reconnect | Unlimited | Retries indefinitely; `plugin_disconnected` notification remains active |
| HTTP transfer request (Tier 4) | 3 | Returns `EIO` to the VFS caller; next file access retries from scratch |
| CouchDB replication | Managed by CouchDB internally | Agent monitors `_replication_state` and writes notification on persistent error |
| Agent heartbeat | Unlimited | Continues retrying; control plane marks node offline after 3 missed polls (90 seconds) |
| Storage backend polling | 3 per poll cycle | Logged as soft error; next poll cycle retries. Persistent failures (e.g. expired OAuth) generate a notification |

The last 50 errors per agent are stored in the agent's status document for quick access without log parsing.

---

