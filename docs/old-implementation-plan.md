# MosaicFS — Implementation Plan

*v0.2*

---

## Table of Contents

- [Overview](#overview)
- [Guiding Principles](#guiding-principles)
- [Dependency Map](#dependency-map)
- [Phase 1 — Foundation](#phase-1--foundation)
- [Phase 2 — Control Plane and REST API](#phase-2--control-plane-and-rest-api)
- [Phase 3 — Rule Evaluation Engine](#phase-3--rule-evaluation-engine)
- [Phase 4 — Virtual Filesystem Layer](#phase-4--virtual-filesystem-layer)
- [Phase 5 — Web UI](#phase-5--web-ui)
- [Phase 6 — Plugin System](#phase-6--plugin-system)
- [Phase 7 — Notification System](#phase-7--notification-system)
- [Phase 8 — Backup and Restore](#phase-8--backup-and-restore)
- [Phase 9 — Bridge Nodes](#phase-9--bridge-nodes)
- [Phase 10 — CLI and Desktop App](#phase-10--cli-and-desktop-app)
- [Phase 11 — Hardening and Production Readiness](#phase-11--hardening-and-production-readiness)
- [Testing Strategy](#testing-strategy)
- [Migration Between Phases](#migration-between-phases)
- [Risk Register](#risk-register)
- [Out of Scope for v1](#out-of-scope-for-v1)

---

## Overview

This document describes the build order for MosaicFS v1. Each phase ends with a concrete, testable milestone. Phases are sized for solo part-time development. Later phases depend on earlier ones; skipping ahead is possible but makes debugging harder.

The architecture document is the authoritative reference for all design decisions, schemas, and API contracts. This plan references it but does not repeat it. When the two conflict, the architecture document wins.

---

## Guiding Principles

**Build end-to-end slices, not horizontal layers.** Completing the full path from "agent crawls a file" to "that file appears in the database" in Phase 1 is more valuable than finishing the entire database schema first. Thin vertical slices catch integration problems early.

**Make each phase observable.** Every phase should include enough logging and introspection that you can see what the system is doing without a debugger. Silent correctness is harder to debug in the next phase.

**Test the hard invariants, not the plumbing.** Don't write tests for getters and setters. Do write tests for: inode stability across restarts, transfer `Digest` trailer verification, rule engine evaluation order, and VFS read consistency.

**Defer write complexity.** The VFS layer is read-only in v1. The rule engine evaluates on demand without writing to file documents. Write paths are the hardest part of a distributed filesystem — deferring them is correct, not lazy.

---

## Dependency Map

```
Phase 1: Foundation
  └─► Phase 2: Control Plane & API
        ├─► Phase 3: Rule Engine
        │     ├─► Phase 4: VFS
        │     └─► Phase 5: Web UI
        │           └─► Phase 10: CLI & Desktop
        ├─► Phase 6: Plugin System (backend: parallel with 3–5; UI: after 5)
        │     ├─► Phase 7: Notifications (parallel with 8, 9)
        │     └─► Phase 9: Bridge Nodes (parallel with 7, 8)
        └─► Phase 8: Backup & Restore (after 2 + 5)
  └─► Phase 11: Hardening (continuous from Phase 2 onward)
```

Phase 11 is not a final gate — hardening work should begin as soon as Phase 2 is complete and continue in parallel with everything after it.

---

## Phase 1 — Foundation

**Goal:** An agent that crawls a directory, writes file documents to a local CouchDB, and replicates them to the control plane. No API, no VFS, no rules — just files in the database.

**Milestone:** Run the agent on your laptop, configure it to watch `~/Documents`, and see file documents appear in Fauxton on both the local and control plane CouchDB instances.

### 1.1 — Repository and Project Structure

Set up the Cargo workspace with three crates: `mosaicfs-agent`, `mosaicfs-server`, and `mosaicfs-common`. The common crate holds document type definitions, serialization, and shared utilities.

### 1.2 — Docker Compose Stack

Write the initial `docker-compose.yml` for the control plane containing only CouchDB. Configure an admin user, bind to localhost, create the `mosaicfs` database, and add a healthcheck. Also create `docker-compose.dev.yml` for local development (see [Testing Strategy](#testing-strategy)). Document setup steps in `DEVELOPMENT.md`.

### 1.3 — CouchDB Document Types

Define Rust structs for all eleven v1 document types in `mosaicfs-common` with `serde` serialization. The eleven types are: `file`, `virtual_directory`, `node`, `credential`, `agent_status`, `utilization_snapshot`, `label_assignment`, `label_rule`, `plugin`, `annotation`, `notification`. Pay attention to the `_id` format conventions — they are load-bearing. Include unit tests that round-trip each type through JSON.

### 1.4 — Agent Configuration

Implement `agent.toml` parsing. Required fields: `control_plane_url`, `node_id` (read from file or generated on first run), `watch_paths`, `access_key_id`, `secret_key`. Validate at startup; exit with a clear error if anything is missing.

### 1.5 — Local CouchDB Client

Implement a CouchDB HTTP client in the agent: `get_document`, `put_document`, `bulk_docs`. Use `reqwest`. Don't use a third-party CouchDB crate — the interface is simple enough that a hand-rolled client gives full control over error handling.

### 1.6 — Filesystem Crawler

Walk all configured `watch_paths` using `walkdir`. For each file, stat the path and check `(export_path, size, mtime)` against the existing document — skip unchanged files. Write new or changed documents in batches of 200 via `_bulk_docs`. No content hashing — change detection relies on `size` and `mtime`. Log a summary at completion.

### 1.7 — CouchDB Replication Setup

Configure bidirectional continuous replication between the agent's local CouchDB and the control plane. Use the `_replicator` document structures from the architecture doc (Flow 1 push, Flow 2 pull). At this stage the filters can be simplified; tighten them in Phase 2 when credentials exist. Monitor replication state and log changes.

### 1.8 — Node Document

On startup, write or update the node document: `friendly_name` (defaults to hostname), `platform`, `status: "online"`, `last_heartbeat`. Run heartbeat on a 30-second timer. On clean shutdown, set `status: "offline"`.

### Phase 1 Checklist

- [ ] Agent starts, creates `node_id` file on first run
- [ ] Crawls configured paths and writes `file` documents
- [ ] Stat fast-path skips unchanged files on repeated crawls
- [ ] Documents replicate to control plane CouchDB
- [ ] Node document with heartbeat appears in both databases
- [ ] Agent exits cleanly on SIGTERM with status set to offline

---

## Phase 2 — Control Plane and REST API

**Goal:** The Axum API server runs with TLS, all REST endpoints exist, HMAC and JWT authentication work, and `curl` can query indexed files.

**Milestone:** `curl -H "Authorization: Bearer <token>" https://localhost:8443/api/files` returns a paginated list of files indexed in Phase 1.

### 2.1 — Axum Server Skeleton

Set up the `mosaicfs-server` binary with Axum and TLS (self-signed cert generated at first run). Register all API routes as 501 stubs. Add request logging middleware.

### 2.2 — Credential Management

Implement credential CRUD: create (generate access key ID + secret, hash with Argon2id), list, get, enable/disable, delete. The secret is returned once at creation and never stored in recoverable form.

### 2.3 — JWT Authentication

Implement `POST /api/auth/login` with rate limiting (5 attempts/min/IP). Issue 24-hour JWTs signed with the server's persistent signing key (see architecture doc). Implement Bearer token middleware, `GET /api/auth/whoami`, `POST /api/auth/logout`.

### 2.4 — HMAC Authentication

Implement the HMAC-SHA256 request signing middleware for `/api/agent/` endpoints. Validate canonical signed string, reject timestamps outside ±5 minutes, look up credential by access key ID.

### 2.5 — Node Endpoints

Implement `/api/nodes` CRUD: list, get, register (called by `agent init`), patch, delete (soft disable). Implement `/api/nodes/{node_id}/status` and `/api/nodes/{node_id}/mounts` CRUD.

### 2.6 — File and Search Endpoints

Implement `GET /api/files`, `GET /api/files/{file_id}`, `GET /api/files/by-path`, and `GET /api/search?q=...` (substring and glob on `name`). Verify CouchDB indexes are created at startup.

### 2.7 — Virtual Filesystem Endpoints

Implement `GET /api/vfs?path=...`, `GET /api/vfs/tree`, and directory CRUD (`POST`, `GET`, `PATCH`, `DELETE`). Validate: `virtual_path` format, no `/federation/` prefix, system directories cannot be deleted.

### 2.8 — Agent Internal Endpoints

Implement `/api/agent/` endpoints: heartbeat, bulk file upsert (with per-document success/error handling), status, utilization, credentials, and `GET /api/agent/transfer/{file_id}`. The bulk upsert must handle partial failures — one bad document must not fail the entire batch.

### 2.9 — Agent Init Command

Implement `mosaicfs-agent init`: prompt for control plane URL and credentials (secret from stdin with echo disabled), register node, write `agent.toml`, install systemd unit or launchd plist, start the service.

### 2.10 — File Content Delivery

Implement `GET /api/files/{file_id}/content`. For Phase 2, implement only the local file case and remote agent HTTP fetch (Tier 4). Support `Range` headers and `Content-Disposition`.

### 2.11 — Labels API

Implement `/api/labels` endpoints: list all labels, assignment CRUD (deterministic `_id`, upsert semantics), rule CRUD (validate trailing `/` on prefix, validate `node_id`), and `GET /api/labels/effective`. Extend search to support `?label=` filtering.

### Phase 2 Checklist

- [ ] Axum starts with TLS, all routes registered
- [ ] JWT login, whoami, logout work
- [ ] HMAC authentication validates and rejects correctly
- [ ] Login rate limiting prevents brute force
- [ ] Node registration, listing, and detail endpoints work
- [ ] File listing and search return Phase 1 indexed files
- [ ] Agent bulk upsert handles partial failures
- [ ] File content downloads work for local and remote-agent files
- [ ] Label assignment and rule CRUD work; assignments survive re-indexing
- [ ] `GET /api/labels/effective` returns correct union
- [ ] `GET /api/search?label=` filters correctly

---

## Phase 3 — Rule Evaluation Engine

**Goal:** Virtual directories with mount sources and filter steps return matching files. Files can appear in multiple directories simultaneously.

**Milestone:** Create `/documents/work`, add a mount from `~/Documents` with a glob step for `*.pdf`, and verify `GET /api/vfs?path=/documents/work` returns the expected PDFs.

### 3.1 — Virtual Directory Seeding

Create the root directory document (`dir::root`) at startup if absent.

### 3.2 — Step Pipeline Evaluator

Implement the step pipeline in `mosaicfs-common`. The function takes a mount entry, inherited steps, and a file document, returning include/exclude.

Write thorough unit tests: each op (glob, regex, age, size, mime, node, label, annotation) with and without `invert`; `on_match` short-circuit; `default_result` fallback; empty steps; ancestor inheritance; ancestor `exclude` overriding child `include`.

The `label` op requires the file's effective label set. Implement `resolve_effective_labels` as a lookup against the materialized label cache (see architecture doc). For the unit test context, the cache can be populated from test fixtures.

### 3.3 — Materialized Label Cache

Implement the in-memory label cache as described in the architecture document. Build at agent startup from `label_assignment` and `label_rule` documents. Maintain incrementally via the CouchDB changes feed. The cache must be ready before the VFS mount becomes available.

### 3.4 — Readdir Evaluation

Implement `readdir` evaluation in `mosaicfs-common`:
1. Walk ancestor chain, collect inherited steps
2. For each mount, query files by `(source.node_id, source.export_parent prefix)`
3. Run step pipeline (inherited + mount steps) per file
4. Apply mapping strategy (`prefix_replace` or `flatten`)
5. Apply `conflict_policy` on name collisions (conservative policy wins across mounts)
6. Include child `virtual_directory` documents as subdirectory entries

Test: multi-source merging, both conflict policies, flatten vs prefix_replace, inherited filtering, same file in two directories.

### 3.5 — On-Demand VFS Endpoint

Wire readdir into `GET /api/vfs?path=...` and `GET /api/vfs/tree`.

### 3.6 — Directory Preview Endpoint

Implement `POST /api/vfs/directories/{path}/preview` — evaluates a draft `mounts` configuration without saving.

### 3.7 — Readdir Cache

Implement the short-lived readdir cache (default 5s TTL, keyed by virtual path + document revision). Invalidate via the CouchDB changes feed.

### Phase 3 Checklist

- [ ] Root directory document created at startup
- [ ] Step pipeline passes all unit tests including ancestor inheritance
- [ ] Materialized label cache builds at startup and updates incrementally
- [ ] `label` op uses O(1) cache lookup, not per-file CouchDB query
- [ ] `readdir` queries by `source.export_parent` prefix using the index
- [ ] `prefix_replace` and `flatten` produce correct filenames
- [ ] Both conflict policies work; conservative policy wins across mismatched mounts
- [ ] Same file in two directories shows the same inode
- [ ] `GET /api/vfs?path=...` returns matching files
- [ ] Preview evaluates unsaved draft mounts
- [ ] Readdir cache invalidates on directory document change
- [ ] Deleted files never appear in listings

---

## Phase 4 — Virtual Filesystem Layer

**Goal:** FUSE mount works. `ls`, `cat`, `cp` all work on local and remote files. The file cache populates on first access and serves subsequent reads.

**Milestone:** Mount the filesystem, `ls /mnt/mosaicfs/documents`, open a PDF from a remote agent in a viewer, confirm the second open serves from cache.

### 4.1 — VFS Common Crate

Create `mosaicfs-vfs`. Define the OS backend trait (`readdir`, `lookup`, `open`, `read`, `getattr`). The common crate owns the readdir evaluator (moved from `mosaicfs-common`), tiered access, and file cache.

### 4.2 — FUSE Backend

Set up `fuser` integration. Implement `lookup`, `getattr`, `readdir` delegating to `mosaicfs-vfs`. First sub-milestone: an empty mount that responds to `ls`.

### 4.3 — Inode Resolution

Implement inode lookup from the local CouchDB replica. Verify stability across restarts. Inode space: 0 invalid, 1 root, 2–999 reserved, 1000+ randomly assigned.

### 4.4 — Local File Access (Tier 1)

Implement `open` and `read` for files on this node. Verify the file exists at `source.export_path`; return `ENOENT` if stale. Validate that the canonicalized path is under a configured watch path (export_path containment check).

### 4.5 — Full-File Cache

Implement the cache at `/var/lib/mosaicfs/cache/`. Create SQLite `index.db`. Cache keys via SHA-256 of `{node_id}::{export_path}`. Downloads go to `cache/tmp/`, atomic rename on completion. Staleness check: compare `mtime` and `size` against file document. Full-file mode for files below the size threshold (default 50 MB).

### 4.6 — Block-Mode Cache

Implement block mode for large files (video/audio streaming). Block map as a sorted `Vec<Range<u64>>` of present intervals, serialized as binary blob in SQLite. Implement: presence check (binary search), missing range calculation, interval insert with merge. Sparse file writes. Coalesce adjacent missing sub-ranges before issuing HTTP range requests. Fragmentation guard: promote to full-file download if intervals exceed 1,000.

Write unit tests for all block map operations.

### 4.7 — Remote File Fetch (Tier 4)

Implement the transfer discovery sequence: file doc → node doc → `transfer.endpoint` → HMAC-signed request. Full-file mode: stream to staging, verify `Digest` trailer, move to final location. Block mode: `Range` request, write to sparse file, update block map in a single SQLite transaction. Implement download deduplication via `Shared` futures.

### 4.8 — Network Mount Tiers (2 and 3)

Implement Tier 2 (CIFS/NFS): check node document for `network_mounts` entry covering the file, translate path, open locally. Implement Tier 3 (iCloud/Google Drive local sync): same check for `icloud_local`/`gdrive_local` mount types; add iCloud eviction detection via extended attribute, fall through to Tier 4 if evicted.

### 4.9 — Cache Eviction

LRU eviction using `cached_bytes` and `last_access` in `index.db`. After each cache write, check total size against cap (default 10 GB) and free space minimum (default 1 GB). Evict in ascending `last_access` order.

### 4.10 — Filesystem Watcher

Implement the `notify`-based watcher. Start after initial crawl. Debounce events over 500ms per path. Correlate renames into a single update. Event storm throttling: switch to full crawl if events exceed 1,000/sec for 5 seconds.

### 4.11 — Reconciliation After Reconnect

Detect reconnection via CouchDB replication state. Run expedited full crawl (mtime/size fast-path) before resuming watch mode.

### Phase 4 Checklist

- [ ] `mosaicfs-vfs` crate exists; readdir evaluator moved into it
- [ ] FUSE mount works, `ls /mnt/mosaicfs` returns results
- [ ] `getattr` returns correct metadata
- [ ] Inodes stable across restarts
- [ ] Full-file cache downloads and serves correctly
- [ ] `Digest` trailer verification rejects corrupted downloads
- [ ] Block map unit tests pass
- [ ] Block mode fetches only requested ranges
- [ ] Adjacent missing blocks coalesced into single range request
- [ ] Concurrent reads share one in-flight fetch
- [ ] Cache eviction respects size and free space constraints
- [ ] Network mount tiers (2, 3) work
- [ ] Watcher detects changes within ~1 second
- [ ] Renames produce single update, not delete+create
- [ ] Event storm triggers full crawl instead of per-event processing
- [ ] Reconciliation crawl runs after reconnect

---

## Phase 5 — Web UI

**Goal:** All pages implemented with PouchDB live sync. The mount editor with live preview works end-to-end.

**Milestone:** Create a rule in the step editor, watch the live preview populate, save it, navigate to the File Browser, download a file from a remote node.

### 5.1 — Project Setup

Initialize React + Vite inside the `mosaicfs-server` static directory. Install shadcn/ui, TanStack Query, PouchDB. Configure Vite to proxy API calls in development. Set up routes for all pages.

### 5.2 — Authentication Shell

Login page, auth context. On login, receive JWT + CouchDB session token for `mosaicfs_browser`. Hold both in memory only. Auth guard on all routes.

### 5.3 — PouchDB Sync

Configure pull-only PouchDB replication using the `mosaicfs_browser` session token. PouchDB becomes the source of truth for document-level data; TanStack Query reads from PouchDB. Direct API calls reserved for mutations and non-document endpoints.

### 5.4 — Navigation Shell

Sidebar with all pages, top bar with instance name and user menu. Responsive collapse to bottom tabs. Shared components: node badge (colored pill) and label chip (solid for direct, outlined for inherited).

### 5.5 — Dashboard

Node health strip, error banner, search bar with keyboard shortcut, system totals, recent activity feed.

### 5.6 — Nodes Page

List view with kind filter. Physical agent detail: status, storage topology, utilization trend chart, watch paths, network mounts CRUD, errors. Cloud bridge detail: OAuth status, sync controls, storage.

### 5.7 — File Browser

Two-panel: lazy-loaded directory tree + sortable contents table. Breadcrumbs, inline filter, file detail drawer (metadata, label editing with direct/inherited distinction, download, inline preview for images/PDF/text). Right-click context menu including "Apply labels to folder" shortcut.

### 5.8 — Search Page

Debounced search bar, label filter chips (ANDed with query), result list with infinite scroll, file detail drawer reuse.

### 5.9 — Labels Page

Two tabs: Assignments (sortable table from PouchDB, path filter, click to open file drawer) and Rules (table with enable/disable toggle, rule editor drawer with live preview).

### 5.10 — Virtual Filesystem Page

Two-panel: directory tree with mount badges + contents table. Directory CRUD. Mount editor drawer: `enforce_steps_on_children` toggle, mount source cards, step pipeline editor (all ops including label and annotation), live preview panel calling the preview endpoint with 500ms debounce. Delete confirmation with cascade warning.

### 5.11 — Storage Page

Utilization table with color-coded bars, per-node trend charts with date range picker.

### 5.12 — Settings Page

Four tabs. Credential management (create with one-time secret display, enable/disable, delete). Cloud bridge OAuth cards (stubs, wired in Phase 9). Plugin settings tab (stubs, wired in Phase 6). General configuration. About tab with reindex trigger. PouchDB replica size display.

### Phase 5 Checklist

- [ ] Login, JWT auth, protected routes work
- [ ] PouchDB syncs all document types, live updates visible
- [ ] Node health strip updates on heartbeats
- [ ] File Browser tree lazy-loads, breadcrumbs work
- [ ] File detail drawer shows labels with direct/inherited distinction
- [ ] Label editing works (add, remove direct; inherited shows rule name)
- [ ] "Apply labels to folder" pre-fills rule editor
- [ ] File download from remote node works
- [ ] Inline preview renders images, PDF, text
- [ ] Search with label filters works correctly
- [ ] Labels page rule toggle updates immediately
- [ ] Rule editor live preview shows correct file count
- [ ] VFS mount editor handles all step ops
- [ ] Mount live preview updates as configuration changes
- [ ] Inherited ancestor steps shown read-only in child editor
- [ ] Credential create shows secret once
- [ ] Plugin settings tab present (stub)
- [ ] Notification bell present (stub)
- [ ] OAuth bridge cards present (stub)
- [ ] Settings page shows PouchDB replica size

---

## Phase 6 — Plugin System

**Goal:** Executable and socket plugins process file events, write annotations, and respond to queries.

**Milestone:** Deploy an AI summarizer (executable) that annotates PDFs. Deploy a fulltext search plugin (socket) that indexes into Meilisearch and responds to queries. Both survive agent restarts.

**Dependencies:** Backend (6.1–6.6) can be built immediately after Phase 2, in parallel with Phases 3–5. UI integration (6.7) requires Phase 5.

### 6.1 — Plugin Document Type and Configuration

Add `plugin` and `annotation` document types to `mosaicfs-common`. Implement replication filters (agents receive only their own node's plugins). Add plugin CRUD endpoints. Add plugin directory enumeration to `agent_status`.

### 6.2 — Plugin Job Queue

Create `plugin_jobs.db` with the SQLite schema from the architecture doc. Enqueue jobs on `file.added`/`modified`/`deleted` from the watcher. Implement backoff, `max_attempts`, status tracking. Implement queue size cap (100K per plugin) with notification on overflow. Purge completed/failed jobs after 24 hours.

### 6.3 — Executable Plugin Runner

Implement the full invocation contract from the architecture doc: resolve name to platform plugin directory, construct event envelope, invoke via `execv` (not shell), read stdout JSON, write annotation document. Worker pool with configurable concurrency. Exit 0 = success, non-zero = retry, exit 78 = permanent error. Stdout limit 10 MB, stderr captured at WARN. SIGTERM then SIGKILL on timeout.

### 6.4 — Socket Plugin Support

Connect to `/run/mosaicfs/plugin-sockets/{name}.sock`. Implement newline-delimited JSON with sequence-numbered ack protocol. Replay unacknowledged jobs on reconnect. Exponential backoff on disconnect.

### 6.5 — Plugin Full Sync

Implement `POST /api/nodes/{node_id}/plugins/{plugin_name}/sync`. Compare `annotation.annotated_at` vs `file.mtime`, skip current files. Emit `sync.started`/`sync.completed` events.

### 6.6 — Plugin Query Routing

Implement `query_endpoints` on plugin documents. Agent advertises capabilities on node document. `POST /api/query` fans out by capability. `POST /api/agent/query` delivers queries from control plane to agent.

### 6.7 — Web UI Integration

Add Plugins tab to Settings (render forms from `settings_schema`). Add Annotations section to file detail drawer. Add plugin status to node detail. Add plugin results to Search page.

### Phase 6 Checklist

- [ ] Plugin documents replicate to agents
- [ ] Executable plugin processes PDF, writes annotation
- [ ] Socket plugin connects, receives events, acks correctly
- [ ] Job queue survives agent restart
- [ ] Queue cap enforced; notification on overflow
- [ ] Full sync skips current annotations, processes stale files
- [ ] Query routing fans out to nodes advertising capability
- [ ] Settings page renders plugin forms from schema
- [ ] Annotations appear in file detail drawer

---

## Phase 7 — Notification System

**Goal:** System events appear as notification documents and reach the browser in real time via PouchDB.

**Milestone:** Fill a watched volume to trigger a storage warning, see it in the notification bell within seconds, acknowledge it, watch the badge clear.

**Dependencies:** Requires Phase 6 for plugin health checks. Parallel with Phases 8 and 9.

### 7.1 — Notification Document Type

Add `notification` document type with deterministic `_id` deduplication. Add to all three replication flows. Create CouchDB index on `(type, status, severity)`.

### 7.2 — Agent Notification Writers

Implement notifications for: first crawl complete (info), inotify limit approaching (warning, auto-resolve), cache near capacity (warning, auto-resolve), storage near capacity (warning, auto-resolve), watch path inaccessible (error, auto-resolve), plugin disconnected (warning, resolve on reconnect), replication error (error), auth timestamp rejected (error), plugin queue full (warning).

### 7.3 — Plugin Health Check Polling

Health check messages over socket on configurable interval (default 5 min). Parse `notifications[]` and `resolve_notifications[]` from response. Write notification documents on plugin's behalf. Write `plugin_health_check_failed` after 3 missed checks.

### 7.4 — Control Plane Notifications

New node registered (info), credential inactive (warning), CouchDB replication stalled (warning), control plane disk low (warning), persistent CouchDB conflicts (warning).

### 7.5 — Notification REST API

`GET /api/notifications` (with status/severity filters), `POST /api/notifications/{id}/acknowledge`, `POST /api/notifications/acknowledge-all`, `GET /api/notifications/history`.

### 7.6 — Web UI Notification Bell

Bell icon in top nav with unread count badge (red for errors, amber for warnings). Notification panel: severity-grouped, action buttons, acknowledge controls. Dashboard alert banner for active errors. Live updates via PouchDB changes feed.

### Phase 7 Checklist

- [ ] Agent writes notifications to CouchDB on relevant events
- [ ] Plugin health check polling works over socket
- [ ] Control plane writes system-level notifications
- [ ] Notifications replicate to browser via PouchDB
- [ ] Bell icon shows correct unread count
- [ ] Notification panel renders and updates live
- [ ] Acknowledge updates document status
- [ ] Dashboard alert banner appears for active errors

---

## Phase 8 — Backup and Restore

**Goal:** Download minimal or full backups as JSON; restore into a fresh instance.

**Milestone:** Take a minimal backup, destroy the Compose stack, recreate it, restore the backup, see virtual directories and plugin configs reappear. Agents reconnect and re-crawl.

**Dependencies:** Requires Phase 2 (API) and Phase 5 (UI). Independent of plugins/notifications.

### 8.1 — Backup Generation

`GET /api/system/backup?type=minimal` — essential documents only, with `secret`-typed plugin settings redacted to `"__REDACTED__"`. `GET /api/system/backup?type=full` — all documents. Both stream as `Content-Disposition: attachment` JSON in `_bulk_docs` format.

### 8.2 — Restore Process

`POST /api/system/restore` — validate JSON, check document types, bulk write. Only permitted into an empty database. For minimal backups: extract `network_mounts` from partial node documents, merge via PATCH. Return `{ restored_count, errors }`.

### 8.3 — Developer Mode

`--developer-mode` flag on control plane (default off). Enables `DELETE /api/system/data` for database wipes during development.

### 8.4 — Web UI Backup Controls

Settings → About: download buttons (minimal/full). Restore section visible only when database is empty. Post-restore banner: "Restart all agents."

### Phase 8 Checklist

- [ ] Minimal backup contains essential documents only
- [ ] Secret settings redacted in backup files
- [ ] Full backup contains complete database
- [ ] Restore into empty database succeeds
- [ ] Network mounts merged correctly for minimal restore
- [ ] DELETE endpoint requires developer mode flag
- [ ] Settings page backup/restore controls work
- [ ] Post-restore, agents reconnect and re-crawl

---

## Phase 9 — Bridge Nodes

**Goal:** A bridge node with a `provides_filesystem` plugin indexes external data and serves files via Tier 1 or Tier 5.

**Milestone:** Configure an email bridge with Gmail OAuth. Watch it index 30 days of email as `.eml` files. Open an email through the VFS mount.

**Dependencies:** Requires Phase 6 (Plugin System). Parallel with Phases 7 and 8.

### 9.1 — Bridge Node Concept

Add `role: "bridge"` to node documents (omitted for physical nodes). Agent detects empty `watch_paths` and skips filesystem crawl. Deliver `crawl_requested` events to `provides_filesystem` plugins.

### 9.2 — Plugin Filesystem Implementation

Add `provides_filesystem` and `file_path_prefix` to plugin document. Implement the `crawl_requested` stdin/stdout contract: plugin returns a list of file operations, agent applies via `_bulk_docs`.

### 9.3 — Bridge Storage

Docker volume at `/var/lib/mosaicfs/bridge-data` with `files/` (export tree) and `plugin-state/` (opaque). Agent serves files from `files/` via Tier 1.

### 9.4 — Tier 5 Materialize

Implement `materialize` event for Option B bridges. Transfer server checks `file_path_prefix` match, invokes plugin with staging path in `cache/tmp/`, moves result to VFS cache. Add `source` column to cache SQLite schema.

### 9.5 — Cloud Service Bridges

Implement cloud bridges as `provides_filesystem` plugins:

**S3 bridge** — simplest; start here as the reference implementation. Poll `ListObjectsV2`, simulate directories from key prefixes, fetch via streaming response with `Digest` trailer.

**B2 bridge** — S3-compatible API with custom endpoint. Share as much implementation as possible with S3.

**Google Drive bridge** — OAuth2 with refresh tokens, delta sync via Changes API.

**OneDrive bridge** — OAuth2 via Microsoft Graph API, delta sync, path-to-item-ID mapping.

**iCloud bridge** — crawl local `~/Library/Mobile Documents/` sync directory. No API; eviction detection via extended attribute.

### 9.6 — Email Bridge Plugin (Reference)

Gmail OAuth flow, `crawl_requested` handler polling Gmail API, date-based sharding, settings schema (client_id, client_secret, fetch_days, auto_delete_days).

### 9.7 — Bridge Storage Monitoring

Hourly inode and disk utilization check. Write `inodes_near_exhaustion` and `storage_near_capacity` notifications.

### 9.8 — Web UI Bridge Support

Detect `role: "bridge"` on node document. Render "Bridge Storage" section instead of storage topology. Show retention configuration and OAuth controls.

### Phase 9 Checklist

- [ ] Bridge node runs in Docker Compose with volume
- [ ] Plugin receives `crawl_requested`, agent creates file documents
- [ ] Files served via Tier 1 from bridge storage
- [ ] Tier 5 materialize works for Option B
- [ ] S3 bridge indexes bucket and files accessible via VFS
- [ ] At least one OAuth bridge (Google Drive or OneDrive) completes flow
- [ ] Email bridge fetches Gmail, writes `.eml` files
- [ ] Inode monitoring writes notifications
- [ ] Web UI shows bridge-specific controls

---

## Phase 10 — CLI and Desktop App

**Goal:** `mosaicfs-cli` covers common management tasks. The Tauri desktop app wraps the web UI with native integration.

**Milestone:** `mosaicfs-cli files fetch /documents/report.pdf --output ~/Downloads/` downloads a file. The desktop app can drag a file to Finder.

### 10.1 — CLI Foundation

Create `mosaicfs-cli` in the workspace. Load config from `~/.config/mosaicfs/cli.toml`. JWT authentication with in-memory caching. `clap` for argument parsing. Default human-readable output; `--json` for scripting. `--quiet` and `--verbose` flags.

### 10.2 — CLI Commands

```
mosaicfs-cli nodes list | status <node-id>
mosaicfs-cli files search <query> | stat <file-id> | fetch <file-id> [--output <path>]
mosaicfs-cli vfs ls | tree | mkdir | rmdir | show | edit <virtual-path>
mosaicfs-cli storage overview | history <node-id> [--days 30]
mosaicfs-cli credentials create --name <name> | list | revoke <key-id>
mosaicfs-cli system health | reindex
```

### 10.3 — Tauri Desktop App

Wrap the React frontend in Tauri. Native additions: persistent window state, system tray, native file dialogs, drag-to-Finder. Read-only in v1.

### Phase 10 Checklist

- [ ] CLI authenticates and maintains JWT
- [ ] All commands work with human and JSON output
- [ ] `files fetch` downloads with progress indication
- [ ] Tauri builds on macOS and Linux
- [ ] System tray and drag-to-Finder work on macOS

---

## Phase 11 — Hardening and Production Readiness

**Goal:** Graceful failure handling, automatic recovery from transient errors, acceptable performance at target scale, actionable observability.

This phase runs continuously from Phase 2 onward, not as a final gate.

### 11.1 — Error Classification and Retry

Implement the standardized retry parameters from the architecture doc: 1s initial delay, 2x multiplier, 60s cap, ±25% jitter. Apply the per-context retry table (plugin jobs, socket reconnect, HTTP transfer, replication, heartbeat, bridge polling).

### 11.2 — Structured Logging

`tracing` with consistent fields: `node_id`, `file_id`, `operation`, `duration_ms`, `error`. INFO in production. Runtime-adjustable log level. 50 MB rotation, 5 files retained.

### 11.3 — Health Checks and Stale Detection

Wire `GET /health` endpoints to real subsystem data. Control plane polls agents every 30 seconds; mark offline after 3 missed checks (90s). On control plane restart, re-poll all nodes. Run conflict monitoring background task (60s interval).

### 11.4 — inotify Limit Handling

Graceful degradation: unwatched directories fall back to nightly crawl. Log warnings near the limit. `agent init` sets `fs.inotify.max_user_watches = 524288` on Linux.

### 11.5 — Large File Handling

Verify VFS reads, cache writes, and transfer streaming don't buffer full files in memory. Verify `Digest` trailer computation is streaming.

### 11.6 — Replication Edge Cases

Test: control plane unreachable at startup (queue and retry), reconnect after extended outage (reconciliation crawl), clock skew (log warning if >2 minutes).

### 11.7 — Scale Testing

Seed 500K file documents (target scale). Measure: initial crawl time (100K files on disk), readdir latency (10 mount sources), replication cold-start sync, search latency, cache eviction throughput. For block cache: 10 GB video, random seeks, verify interval count stays under 20 after realistic viewing, measure time-to-first-frame and seek latency.

### 11.8 — Installer Polish

Clean `agent init` prompts, URL validation, success confirmation with mount path. README with prerequisites, control plane setup, and agent installation per platform.

### Phase 11 Checklist

- [ ] Transient errors retry with standardized backoff; permanent errors surface to UI
- [ ] Structured logs have consistent fields
- [ ] Health polling marks offline nodes within 90 seconds
- [ ] inotify exhaustion degrades gracefully
- [ ] Large files stream without full buffering
- [ ] Agent starts correctly when control plane is unreachable
- [ ] Reconciliation runs after extended outage
- [ ] 500K-file scale test passes with acceptable performance
- [ ] `agent init` works end-to-end on macOS and Linux

---

## Testing Strategy

**Unit tests** — test the hard invariants with `#[test]`: document serialization round-trips, step pipeline evaluation, cache key computation, HMAC signatures, block map interval operations, label cache incremental updates.

**Integration tests** — require a real CouchDB via Docker Compose:
- Replication filter correctness: write documents, replicate, verify only expected documents arrive
- Backup/restore round-trip: backup, wipe, restore, verify fidelity
- Plugin invocation: deploy a test binary, trigger events, verify annotations
- Transfer server: two agents, fetch a file peer-to-peer, verify bytes match

**Development environment:**
- `docker-compose.dev.yml` runs CouchDB + control plane
- Local agent configured with `watch_paths` pointing to a test directory
- `scripts/seed-test-data.sh` creates sample files, virtual directories, labels, and plugin configs
- `--developer-mode` flag enables database wipe between test cycles

**Mock mode for bridges** — plugins accept a `mock: true` config flag that generates synthetic files instead of calling real cloud APIs. Enables full pipeline testing without OAuth credentials.

**Performance benchmarks** — Phase 11 seeds 500K file documents and measures crawl time, readdir latency, replication sync, search latency, and cache throughput.

---

## Migration Between Phases

No migration scripts are needed between phases. The CouchDB schema is additive:
- New document types are new documents in the same database
- New fields on existing documents use `Option<T>` in Rust (absent = None)
- New CouchDB indexes are created at startup if they don't exist

If a phase changes the structure of an existing document type, include a one-time startup migration function that detects and rewrites old-format documents. The `--developer-mode` wipe is always available during development.

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| VFS read bugs causing incorrect data | Medium | High | Read-only v1 eliminates write-path bugs. Compare bytes read through VFS against direct source read. |
| `Digest` trailer unsupported by some HTTP clients | Low | Low | Trailer is optional. Only agent-to-agent transfers verify; browsers trust TLS. |
| CouchDB replication filters misbehaving | Medium | High | Test filters with document fixtures. Log mismatches at WARN. |
| iCloud bridge unreliable (no official API) | High | Low | Documented as best-effort. Eviction fallback is the safety net. |
| OAuth token refresh failures | Medium | Medium | Automatic refresh with retry. Surface expiry in UI before it causes sync failures. |
| inotify watch exhaustion | High | Medium | Graceful degradation to nightly crawl. Installer raises system limit. |
| PouchDB browser replica too large | Low | Medium | Settings page shows size. Warning at 500 MB. Server-side pagination is the future fix. |
| FUSE bindings (`fuser`) lacking features | Low | High | Evaluate API surface before Phase 4. Fallbacks: `fuse-rs` or direct binding. |
| Plugin job queue overwhelming disk | Medium | Medium | 100K cap per plugin. Completed jobs purged after 24h. Notification on overflow. |

---

## Out of Scope for v1

These features are referenced in the architecture but not implemented in v1:

- **VFS write support** — read-only filesystem. Largest v2 item.
- **Federation** — schema accommodations in place, no implementation.
- **Full-text content search** — filename only. Content indexing needs a search engine.
- **Multi-user support** — single user per instance. Path to multi-user is federation.
- **File mutation API** — REST API is read-only for files.
- **Metadata search filters** — search accepts only a query string.
- **Windows agent** — macOS and Linux only.
- **Tauri mobile** — PWA covers mobile.
- **Automatic backup** — on-demand only; scriptable via `curl`.
- **Key rotation** — JWT signing key is stable for the deployment lifetime.
- **Plugin sandboxing** — plugins run with agent privileges; plugin directory is the trust boundary.

---

*MosaicFS Implementation Plan v0.2*
