# MosaicFS Implementation Plan — Required Updates

This document describes changes needed to bring the implementation plan into sync with the current architecture document (v0.1, 2,614 lines).

## Document Count Correction

**Current:** "Define Rust structs for all eight v1 document types"  
**Should be:** Eleven document types in v1: `file`, `virtual_directory`, `node`, `credential`, `agent_status`, `utilization_snapshot`, `label_assignment`, `label_rule`, `plugin`, `annotation`, `notification`.

**Fix applied:** Phase 1.3 updated to mention core types for Phase 1, with additional types added in later phases.

---

## New Phases to Add

The current plan has 8 phases. The updated plan should have 11 phases with the following additions:

### Phase 6 — Plugin System (NEW)

**Goal:** Executable and socket plugins can process file events, write annotations back to CouchDB, and integrate with the agent's job queue and replication system.

**Milestone:** Deploy an AI summarization plugin (executable) that processes PDFs, writes summaries as annotations, and survives agent restarts with its job queue intact. Deploy a fulltext search plugin (socket) that indexes files into Meilisearch and responds to queries.

**Substeps:**

6.1 — Plugin Document Type and Configuration  
- Add `plugin` and `annotation` document types to `mosaicfs-common`
- Implement plugin document replication filters (agents only receive their own node's plugin configs)
- Add `GET /api/nodes/{node_id}/plugins` and related CRUD endpoints
- Add plugin directory enumeration to `agent_status` (reports available plugins)

6.2 — Plugin Job Queue (SQLite)  
- Create `plugin_jobs.db` with schema from architecture doc
- Implement job enqueue on `file.added`, `file.modified`, `file.deleted` from watcher
- Implement exponential backoff, `max_attempts`, status tracking

6.3 — Executable Plugin Runner  
- Implement plugin invocation: resolve name to platform directory, construct event envelope, invoke via stdin/stdout
- Implement worker pool (configurable `workers` count per plugin)
- Parse stdout JSON, write to `annotation` document on success
- Handle failures: increment attempts, permanent error after `max_attempts`

6.4 — Socket Plugin Support  
- Implement Unix socket connection to `/run/mosaicfs/plugin-sockets/{name}.sock`
- Implement ack protocol with sequence numbers
- Replay unacknowledged jobs from SQLite queue on reconnect
- Handle disconnect: mark jobs `pending`, retry with exponential backoff

6.5 — Plugin Full Sync  
- Implement `POST /api/nodes/{node_id}/plugins/{plugin_name}/sync` endpoint
- Compare `annotation.annotated_at` vs `file.mtime`, skip current annotations
- Emit `sync.started` and `sync.completed` events

6.6 — Plugin Query Routing  
- Implement `query_endpoints` declaration in plugin document
- Update node document with `capabilities` array (advertised by agent when plugin has query endpoints)
- Implement `POST /api/query` with capability-based fan-out
- Implement `POST /api/agent/query` for control plane → agent query delivery

6.7 — Web UI Integration  
- Add Plugins tab to Settings page (render forms from `settings_schema`)
- Add Annotations section to file detail drawer
- Add plugin status to node detail page
- Add plugin results section to Search page

**Completion checklist:**
- [ ] Plugin documents replicate to agents
- [ ] Executable plugin processes PDF, writes annotation
- [ ] Socket plugin connects, receives events, responds with acks
- [ ] Job queue survives agent restart
- [ ] Full sync skips current annotations, processes stale files
- [ ] Query routing fans out to nodes advertising capability
- [ ] Settings page renders plugin forms from schema
- [ ] Annotations appear in file detail drawer

**Dependencies:** Substeps 6.1–6.6 (plugin backend) can be built immediately after Phase 2, in parallel with Phases 3–5. Substep 6.7 (Web UI integration) requires Phase 5 complete. Recommended approach: build the plugin backend during Phases 3–4, then wire up UI integration after Phase 5.

---

### Phase 7 — Notification System (NEW)

**Goal:** System events from agents, plugins, and the control plane appear as notification documents in CouchDB and are delivered to the browser in real time via PouchDB.

**Milestone:** Trigger a storage capacity warning by filling a watched volume, see the notification appear in the web UI notification bell within seconds, acknowledge it, and see the bell badge clear.

**Substeps:**

7.1 — Notification Document Type  
- Add `notification` document type with deterministic `_id` deduplication scheme
- Add notification replication to all three flows (agent push, control plane pull, browser pull)
- Add CouchDB index on `type`, `status`, `severity`

7.2 — Agent Notification Writers  
- First crawl completion: write `first_crawl_complete` (info)
- Inotify limit check (hourly): write/resolve `inotify_limit_approaching`
- Cache pressure check (hourly): write/resolve `cache_near_capacity`
- Storage check (hourly): write/resolve `storage_near_capacity`
- Watch path inaccessible: write/resolve `watch_path_inaccessible:{path}`
- Plugin socket disconnect: write `plugin_disconnected:{plugin_name}`, resolve on reconnect

7.3 — Plugin Health Check Polling  
- Add `health_check_interval_s` to plugin document (default 300s)
- Implement health check message send over socket
- Parse `notifications[]` and `resolve_notifications[]` from response
- Write notification documents on plugin's behalf
- After 3 missed checks: write `plugin_health_check_failed`

7.4 — Control Plane Notifications  
- New node registered: write `new_node_registered:{node_id}` (info)
- Credential inactive: write `credential_inactive:{key_id}` (warning)
- CouchDB replication stalled: write `replication_stalled` (warning)
- Disk low: write `control_plane_disk_low` (warning)

7.5 — Notification REST API  
- `GET /api/notifications` with status/severity filters
- `POST /api/notifications/{id}/acknowledge`
- `POST /api/notifications/acknowledge-all`
- `GET /api/notifications/history`

7.6 — Web UI Notification Bell  
- Bell icon in top nav with unread count badge (red for errors, amber for warnings)
- Notification panel: severity-grouped, action buttons, acknowledge controls
- Dashboard alert banner for error-severity notifications
- Live updates via PouchDB changes feed

**Completion checklist:**
- [ ] Agent writes notifications to CouchDB
- [ ] Plugin health check polling invokes socket plugins
- [ ] Control plane writes system-level notifications
- [ ] Notification documents replicate to browser
- [ ] Bell icon shows unread count badge
- [ ] Notification panel renders and updates in real time
- [ ] Acknowledge updates document status
- [ ] Dashboard alert banner appears for active errors

**Dependencies:** Requires Phase 6 (Plugin System) for plugin health checks. Can run in parallel with Phase 8 and 9.

---

### Phase 8 — Backup and Restore (NEW)

**Goal:** Users can download minimal or full backups as JSON files and restore them into a fresh MosaicFS instance.

**Milestone:** Take a minimal backup of a configured MosaicFS instance, destroy the Docker Compose stack, recreate it, restore the backup, and see virtual directories, labels, annotations, and plugin configurations reappear. Agents reconnect and re-crawl files.

**Substeps:**

8.1 — Backup Generation  
- `GET /api/system/backup?type=minimal` — filter to essential documents, stream as JSON
- `GET /api/system/backup?type=full` — all documents via `_all_docs?include_docs=true`
- Implement `_bulk_docs` format generation with filename timestamp

8.2 — Restore Process  
- `GET /api/system/backup/status` — check if database is empty
- `POST /api/system/restore` — validate JSON, check document types, bulk write
- For minimal backups: extract `network_mounts` from partial node docs, merge via PATCH
- Return summary: `{ restored_count, errors }`

8.3 — Developer Mode DELETE Endpoint  
- Add `--developer-mode` flag to control plane binary (default: off)
- Implement `DELETE /api/system/data` (requires confirmation token, gated by flag)
- Returns 403 unless developer mode enabled

8.4 — Web UI Backup Controls  
- Settings → About tab: two download buttons (minimal/full)
- Conditionally show restore section when database empty
- Disabled restore control with tooltip when not empty
- Post-restore banner: "Restart all agents"

**Completion checklist:**
- [ ] Minimal backup downloads JSON with essential documents only
- [ ] Full backup downloads complete database
- [ ] Restore into empty database succeeds
- [ ] Network mounts merged correctly for minimal backup
- [ ] DELETE endpoint requires developer mode flag
- [ ] Settings page backup/restore buttons work
- [ ] Post-restore, agents reconnect and re-crawl

**Dependencies:** Requires Phase 2 (REST API) and Phase 5 (Web UI). Independent of plugins/notifications.

---

### Phase 9 — Bridge Nodes (NEW - replaces old "Cloud Bridges" phase)

**Goal:** A bridge node with an email-fetch plugin running in Docker Compose indexes Gmail messages as files, serves them via Tier 1 or Tier 5, and survives restarts with its bridge storage intact.

**Milestone:** Configure an email bridge node with Gmail OAuth, watch it index the last 30 days of email as `.eml` files under `/gmail/inbox/`, and successfully open an email in the VFS mount.

**Substeps:**

9.1 — Bridge Node Concept  
- Add `role: "bridge"` to node document schema
- Update agent to detect `watch_paths = []` and skip filesystem crawl
- Implement `crawl_requested` event delivery to `provides_filesystem` plugins

9.2 — Plugin Filesystem Implementation  
- Add `provides_filesystem` and `file_path_prefix` to plugin document
- Implement `crawl_requested` stdin/stdout contract (plugin returns file operations list)
- Agent applies file operations via `_bulk_docs`

9.3 — Bridge Storage Setup  
- Docker volume mounted at `/var/lib/mosaicfs/bridge-data`
- Separate `files/` (export tree) and `plugin-state/` (opaque to MosaicFS)
- Agent serves files from `files/` via Tier 1 (local file)

9.4 — Tier 5 Materialize (Option B)  
- Add `materialize` event type
- Implement transfer server check for `file_path_prefix` match
- Invoke plugin with staging path in `cache/tmp/`
- Plugin writes bytes, agent moves to VFS cache
- Add `source` column to cache SQLite schema

9.5 — Email Bridge Plugin (Reference Implementation)  
- Implement Gmail OAuth flow and token storage in `plugin-state/`
- Implement `crawl_requested` handler: poll Gmail API, write `.eml` files to `files/gmail/`
- Implement date-based sharding (`files/gmail/2026/02/16/message.eml`)
- Implement settings schema: client_id, client_secret, fetch_days, auto_delete_days

9.6 — Bridge Storage Monitoring  
- Add inode utilization check (hourly)
- Write `inodes_near_exhaustion` notification when approaching limit
- Format bridge volumes with high inode count (`mkfs.ext4 -N 2000000`)

9.7 — Web UI Bridge Node Support  
- Detect `role: "bridge"` in node document
- Render "Bridge Storage" section instead of "Storage Topology"
- Show retention configuration, not disk/volume topology

**Completion checklist:**
- [ ] Bridge node runs in Docker Compose with volume
- [ ] Plugin with `provides_filesystem` receives `crawl_requested`
- [ ] Plugin writes files to `files/`, agent creates file documents
- [ ] Files served via Tier 1 from bridge storage
- [ ] Tier 5 materialize works for Option B plugins
- [ ] Email bridge fetches Gmail, writes `.eml` files
- [ ] Inode monitoring writes notifications
- [ ] Web UI shows bridge-specific controls

**Dependencies:** Requires Phase 6 (Plugin System) complete. Can run in parallel with Phase 7 and 8.

---

## Renumber Remaining Phases

**Old Phase 7 (CLI and Desktop App)** → **New Phase 10**  
**Old Phase 8 (Hardening)** → **New Phase 11**

No content changes needed, just renumber.

---

## Update Phase 5 Checklist

Phase 5 (Web UI) checklist currently says:
```
- [ ] OAuth bridge cards present (stubs for now, wired in Phase 6)
```

Should say:
```
- [ ] OAuth bridge cards present (stubs for now, wired in Phase 9)
- [ ] Plugin settings tab present (stubs for now, wired in Phase 6)
- [ ] Notification bell present (stubs for now, wired in Phase 7)
```

---

## Testing Strategy

The original implementation plan says "test the hard invariants, not the plumbing." This section expands on that with concrete guidance.

**Unit tests** — each phase's completion checklist implies unit test coverage. Test document serialization round-trips, rule engine evaluation with known inputs, cache key computation, HMAC signature generation/validation, and block map interval operations. Use `#[test]` in Rust with no external dependencies.

**Integration tests** — require a real CouchDB instance. Use a Docker Compose test environment with a throwaway CouchDB container. Key integration tests:
- Replication filter correctness: write documents, replicate, verify only expected documents arrive
- Backup/restore round-trip: backup, wipe, restore, verify document fidelity
- Plugin invocation: deploy a test plugin binary, trigger events, verify annotations written
- Transfer server: start two agents, request a file from one to the other, verify bytes match

**Development environment** — a single-machine setup for local development:
- `docker-compose.dev.yml` runs CouchDB + control plane
- A local agent instance configured with `watch_paths` pointing to a test directory
- A `scripts/seed-test-data.sh` script that creates sample files, virtual directories, labels, and plugin configurations
- `--developer-mode` flag enables database wipe between test cycles

**Mock mode for cloud bridges** — bridge plugins should accept a `mock: true` config flag that generates synthetic files instead of calling real cloud APIs. This enables testing the bridge pipeline end-to-end without OAuth credentials.

**Performance benchmarks (Phase 11)** — seed CouchDB with 500K file documents and measure:
- Full crawl time for 100K files on disk
- `readdir` latency for a directory with 10 mount sources
- Replication sync time from cold start
- Search query latency
- Cache eviction throughput

---

## Migration Between Phases

Each phase builds on the previous database state. No migration scripts are needed between phases — the CouchDB schema is additive:
- New document types are simply new documents in the same database
- New fields on existing documents use `Option<T>` in Rust (absent = None)
- New CouchDB indexes are created at startup if they don't exist

If a phase changes the structure of an existing document type (unlikely but possible), the phase's implementation notes should include a one-time migration function that runs at startup, detects old-format documents, and rewrites them. The `--developer-mode` database wipe is always available as a fallback during development.

---

## Summary

**New structure:**
1. Foundation
2. Control Plane & API
3. Rule Engine
4. VFS
5. Web UI
6. Plugin System (NEW)
7. Notification System (NEW)
8. Backup & Restore (NEW)
9. Bridge Nodes (NEW - replaces cloud bridges with updated architecture)
10. CLI & Desktop (was 7)
11. Hardening (was 8)

**Key architectural updates reflected:**
- Plugin system with executable + socket types
- Annotations as first-class documents
- Notifications with real-time PouchDB delivery
- Backup/restore with developer mode
- Bridge nodes as generalized data source adapters
- Option A (files on disk) vs Option B (aggregate storage + materialize)
- Plugin query routing with capability advertisement
- Plugin health checks with notification integration

**Document count:** Updated from 8 to 11 types total.
