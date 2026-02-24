# MosaicFS — Implementation Plan

*v0.5*

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
- [Phase 6 — Replication](#phase-6--replication)
- [Phase 7 — Notification System](#phase-7--notification-system)
- [Phase 8 — Backup and Restore](#phase-8--backup-and-restore)
- [Phase 9 — CLI and Desktop App](#phase-9--cli-and-desktop-app)
- [Phase 10 — Hardening and Production Readiness](#phase-10--hardening-and-production-readiness)
- [Testing Strategy](#testing-strategy)
- [Migration Between Phases](#migration-between-phases)
- [Risk Register](#risk-register)
- [Out of Scope for v1](#out-of-scope-for-v1)

---

## Overview

This document describes the build order for MosaicFS v1. Each phase ends with a concrete, testable milestone. Phases are sized for a single developer working sequentially. Later phases depend on earlier ones; skipping ahead is possible but makes debugging harder.

The architecture document is the authoritative reference for all design decisions, schemas, and API contracts. This plan references it but does not repeat it. When the two conflict, the architecture document wins.

**Changes from v0.2:** Added replication as a dedicated phase, separated storage backends/bridge nodes, expanded document types from 11 to 15 (adding `storage_backend`, `replication_rule`, `replica`, `access`), and added the materialized access cache.

**Changes from v0.3:** Moved replication (Phase 6) ahead of plugins and removed the incorrect dependency on the plugin system. Replication is core agent functionality with storage backend adapters as thin Rust trait implementations. Notifications (Phase 7) and Backup/Restore (Phase 8) renumbered accordingly.

**Changes from v0.4:** Plugins are a v2 delivery item and removed from v1 scope entirely. This removes Phase 9 (Plugin System) and Phase 10 (Storage Backends & Bridge Nodes, which depended on plugin infrastructure for source-mode and bridge functionality). CLI and Desktop (Phase 9) and Hardening (Phase 10) renumbered. Source-mode storage backends, bridge nodes, Tier 5 materialize, and all plugin-related web UI are added to Out of Scope. Replication targets (S3, B2, directory, agent) remain in Phase 6 as they are Rust trait implementations with no plugin system dependency.

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
        │           └─► Phase 9: CLI & Desktop
        ├─► Phase 6: Replication (after 2 + 3; Tier 4b after 4)
        ├─► Phase 7: Notifications (after 2; parallel with 6, 8)
        └─► Phase 8: Backup & Restore (after 2 + 5)
  └─► Phase 10: Hardening (continuous from Phase 2 onward)
```

Phase 10 is not a final gate — hardening work should begin as soon as Phase 2 is complete and continue in parallel with everything after it.

---

## Phase 1 — Foundation

**Goal:** An agent that crawls a directory, writes file documents to a local CouchDB, and replicates them to the control plane. No API, no VFS, no rules — just files in the database.

**Milestone:** Run the agent on your laptop, configure it to watch `~/Documents`, and see file documents appear in Fauxton on both the local and control plane CouchDB instances.

### 1.1 — Repository and Project Structure

Set up the Cargo workspace with three crates: `mosaicfs-agent`, `mosaicfs-server`, and `mosaicfs-common`. The common crate holds document type definitions, serialization, and shared utilities.

### 1.2 — CouchDB database creation

Connect to the `mosaicfs` database using the environment variables named in
local development (see [Testing Strategy](#testing-strategy)).

### 1.3 — CouchDB Document Types

Define Rust structs for all 15 v1 document types in `mosaicfs-common` with `serde` serialization. The 15 types are: `file`, `virtual_directory`, `node`, `credential`, `agent_status`, `utilization_snapshot`, `label_assignment`, `label_rule`, `plugin`, `annotation`, `notification`, `storage_backend`, `replication_rule`, `replica`, `access`. Pay attention to the `_id` format conventions — they are load-bearing. Include unit tests that round-trip each type through JSON.

Note: `plugin` and `annotation` document structs are defined here for schema completeness and future v2 compatibility. They are not used by any v1 component.

### 1.4 — Agent Configuration

Implement `agent.toml` parsing. Required fields: `control_plane_url`, `node_id` (read from file or generated on first run), `watch_paths`, `access_key_id`, `secret_key`. Also support `excluded_paths` for preventing crawler from indexing cache/replication directories, and `transfer_serve_paths` for allowing the transfer endpoint to serve replication storage without crawling it. Validate at startup; exit with a clear error if anything is missing.

### 1.5 — Local CouchDB Client

Implement a CouchDB HTTP client in the agent: `get_document`, `put_document`, `bulk_docs`, `changes_feed`. Use `reqwest`. Don't use a third-party CouchDB crate — the interface is simple enough that a hand-rolled client gives full control over error handling.

### 1.6 — Filesystem Crawler

Walk all configured `watch_paths` using `walkdir`, skipping `excluded_paths`. For each file, stat the path and check `(export_path, size, mtime)` against the existing document — skip unchanged files. Write new or changed documents in batches of 200 via `_bulk_docs`. No content hashing — change detection relies on `size` and `mtime`. Assign random 64-bit inodes at file creation time. Log a summary at completion.

### 1.7 — Soft Deletes

After crawl completes, mark files as `status: "deleted"` (with `deleted_at` timestamp) when they no longer exist on disk. Never hard-delete file documents. Preserve the inode if a file reappears at the same path.

### 1.8 — CouchDB Replication Setup

Configure bidirectional continuous replication between the agent's local CouchDB and the control plane. Use the `_replicator` document structures from the architecture doc (Flow 1 push, Flow 2 pull). At this stage the filters can be simplified; tighten them in Phase 2 when credentials exist. Monitor replication state and log changes.

### 1.9 — Node Document

On startup, write or update the node document: `friendly_name` (defaults to hostname), `platform`, `status: "online"`, `last_heartbeat`, `storage` array (filesystem metadata from configured watch paths). Run heartbeat on a 30-second timer. On clean shutdown, set `status: "offline"`.

### Phase 1 Checklist

- [ ] Agent starts, creates `node_id` file on first run
- [ ] Crawls configured paths and writes `file` documents with random inodes
- [ ] Stat fast-path skips unchanged files on repeated crawls
- [ ] Deleted files marked with `status: "deleted"`, never hard-deleted
- [ ] Inode preserved when deleted file reappears
- [ ] Documents replicate to control plane CouchDB
- [ ] Node document with heartbeat appears in both databases
- [ ] Agent exits cleanly on SIGTERM with status set to offline

---

## Phase 2 — Control Plane and REST API

**Goal:** The Axum API server runs with TLS, all REST endpoints exist, HMAC and JWT authentication work, and `curl` can query indexed files.

**Milestone:** `curl -H "Authorization: Bearer <token>" https://localhost:8443/api/files` returns a paginated list of files indexed in Phase 1.

### 2.1 — Axum Server Skeleton

Set up the `mosaicfs-server` binary with Axum and TLS (self-signed CA + server certificate generated at first run). Register all API routes as 501 stubs. Add request logging middleware. Bind to port 8443.

### 2.2 — CouchDB Initialization

On first startup: create CouchDB admin credential, create `mosaicfs_browser` read-only role, create all 17 CouchDB indexes (7 control-plane, 10 agent-local), generate and persist JWT signing secret.

### 2.3 — Credential Management

Implement credential CRUD: create (generate access key ID `MOSAICFS_{16_hex}` + secret, hash with Argon2id), list, get, enable/disable, delete. The secret is returned once at creation and never stored in recoverable form.

### 2.4 — JWT Authentication

Implement `POST /api/auth/login` with rate limiting (5 attempts/min/IP). Issue 24-hour JWTs signed with the server's persistent signing key. Implement Bearer token middleware, `GET /api/auth/whoami`, `POST /api/auth/logout`. Failed login returns generic 401.

### 2.5 — HMAC Authentication

Implement the HMAC-SHA256 request signing middleware for `/api/agent/` endpoints. Canonical string: `METHOD + PATH + ISO8601_TIMESTAMP + SHA256(body)`. Validate timestamps within ±5 minutes. Look up credential by access key ID.

### 2.6 — CouchDB Replication Proxy

Axum proxies CouchDB replication requests from agents through `/api/agent/replicate`. Agents authenticate with HMAC; the proxy forwards to CouchDB with admin credentials. This keeps CouchDB bound to localhost.

### 2.7 — Replication Filters

Tighten replication filters per the architecture doc. Flow 1 (agent push): only `file`, `node`, `agent_status`, `utilization_snapshot`, `annotation`, `access`, `replica`, `notification`. Flow 2 (agent pull): all document types except `agent_status` and `utilization_snapshot`. Flow 3 (browser): exclude `credential` and `utilization_snapshot`.

### 2.8 — Node Endpoints

Implement `/api/nodes` CRUD: list, get, register (called by `agent init`), patch, delete (soft disable). Implement `/api/nodes/{node_id}/status` and `/api/nodes/{node_id}/mounts` CRUD.

### 2.9 — File and Search Endpoints

Implement `GET /api/files`, `GET /api/files/{file_id}`, `GET /api/files/by-path`, and `GET /api/search?q=...` (substring and glob on `name`). Verify CouchDB indexes are created at startup.

### 2.10 — Virtual Filesystem Endpoints

Implement `GET /api/vfs?path=...`, `GET /api/vfs/tree`, and directory CRUD (`POST`, `GET`, `PATCH`, `DELETE`). Validate: `virtual_path` format, no `/federation/` prefix, system directories cannot be deleted.

### 2.11 — Agent Internal Endpoints

Implement `/api/agent/` endpoints: heartbeat, bulk file upsert (with per-document success/error handling), status, utilization, credentials, and `GET /api/agent/transfer/{file_id}`. The bulk upsert must handle partial failures — one bad document must not fail the entire batch.

### 2.12 — Agent Init Command

Implement `mosaicfs-agent init`: prompt for control plane URL and credentials (secret from stdin with echo disabled), register node, write `agent.toml`, install systemd unit or launchd plist, start the service. Set `fs.inotify.max_user_watches = 524288` on Linux.

### 2.13 — File Content Delivery

Implement `GET /api/files/{file_id}/content`. For Phase 2, implement only the local file case and remote agent HTTP fetch (Tier 4). Support `Range` headers and `Content-Disposition`. Include `Digest` trailer (RFC 9530, SHA-256) for full-file responses.

### 2.14 — Labels API

Implement `/api/labels` endpoints: list all labels, assignment CRUD (deterministic `_id` keyed by file UUID, upsert semantics), rule CRUD (validate trailing `/` on prefix, validate `node_id`), and `GET /api/labels/effective`. Extend search to support `?label=` filtering.

### 2.15 — Access Tracking

Implement `access` document writes. Capture points: REST API `GET /api/files/{file_id}/content`, agent transfer endpoint. Debounce: only write if last_access in DB is >1 hour old. Batch writes via `_bulk_docs` every 5 minutes.

### Phase 2 Checklist

- [ ] Axum starts with TLS on port 8443, all routes registered
- [ ] Self-signed CA and server certificate generated on first run
- [ ] CouchDB indexes created at startup
- [ ] JWT login, whoami, logout work
- [ ] HMAC authentication validates and rejects correctly
- [ ] Login rate limiting prevents brute force
- [ ] CouchDB replication proxy works with HMAC auth
- [ ] Replication filters correctly scope documents per flow
- [ ] Node registration, listing, and detail endpoints work
- [ ] File listing and search return Phase 1 indexed files
- [ ] Agent bulk upsert handles partial failures
- [ ] File content downloads work for local and remote-agent files
- [ ] `Digest` trailer present on full-file responses
- [ ] Label assignment and rule CRUD work; assignments keyed by file UUID survive re-indexing
- [ ] `GET /api/labels/effective` returns correct union of direct + rule-based labels
- [ ] Access tracking writes debounced access documents

---

## Phase 3 — Rule Evaluation Engine

**Goal:** Virtual directories with mount sources and filter steps return matching files. Files can appear in multiple directories simultaneously.

**Milestone:** Create `/documents/work`, add a mount from `~/Documents` with a glob step for `*.pdf`, and verify `GET /api/vfs?path=/documents/work` returns the expected PDFs.

### 3.1 — Virtual Directory Seeding

Create the root directory document (`dir::root`) at startup if absent. Deterministic `_id`: `dir::sha256({virtual_path})`.

### 3.2 — Step Pipeline Evaluator

Implement the step pipeline in `mosaicfs-common`. The function takes a mount entry, inherited steps, and a file document, returning include/exclude. Support all 10 step operations: `glob`, `regex`, `age`, `size`, `mime`, `node`, `label`, `access_age`, `replicated`, `annotation`.

Write thorough unit tests: each op with and without `invert`; `on_match` short-circuit (`include`, `exclude`, `continue`); `default_result` fallback; empty steps; ancestor inheritance; ancestor `exclude` overriding child `include`.

Note: the `annotation` step is implemented for future v2 compatibility. In v1 it will never match (no plugin writes annotations), but the implementation cost is trivial and keeps the rule engine complete.

### 3.3 — Materialized Label Cache

Implement the in-memory label cache (`HashMap<file_uuid, HashSet<String>>`). Build at agent startup from `label_assignment` and `label_rule` documents. Maintain incrementally via the CouchDB changes feed:
- `label_assignment` create/update/delete: recompute entry for that file
- `label_rule` create/update/enable: add labels to all matching files
- `label_rule` delete/disable: full recompute from scratch

The cache must be ready before the VFS mount becomes available. Memory cost: ~5–10 MB for 500K files with 10% labeled.

### 3.4 — Materialized Access Cache

Implement the in-memory access cache (`HashMap<file_id, DateTime<Utc>>`). Build at startup from `access` documents. Maintain incrementally via CouchDB changes feed. Debounced persistence: flush every 5 minutes, only write if last_access in DB >1 hour old, batch via `_bulk_docs`. Memory cost: ~3–5 MB for 500K files with 10% accessed.

### 3.5 — Readdir Evaluation

Implement `readdir` evaluation in `mosaicfs-common`:
1. Walk ancestor chain, collect inherited steps (root → parent)
2. For each mount, query files by `(source.node_id, source.export_parent prefix)` using CouchDB index
3. Run step pipeline (inherited + mount steps) per file
4. Apply mapping strategy (`prefix_replace` strips prefix; `flatten` discards hierarchy)
5. Apply `conflict_policy` on name collisions (conservative policy wins across mounts)
6. Include child `virtual_directory` documents as subdirectory entries

Test: multi-source merging, both conflict policies, flatten vs prefix_replace, inherited filtering, same file in two directories, `enforce_steps_on_children` flag.

### 3.6 — On-Demand VFS Endpoint

Wire readdir into `GET /api/vfs?path=...` and `GET /api/vfs/tree`.

### 3.7 — Directory Preview Endpoint

Implement `POST /api/vfs/directories/{path}/preview` — evaluates a draft `mounts` configuration without saving.

### 3.8 — Readdir Cache

Implement the short-lived readdir cache (default 5s TTL, keyed by virtual path + document revision). Invalidate via the CouchDB changes feed when directory document changes.

### Phase 3 Checklist

- [ ] Root directory document created at startup
- [ ] All 10 step operations implemented and tested
- [ ] Step pipeline passes all unit tests including ancestor inheritance
- [ ] Materialized label cache builds at startup and updates incrementally
- [ ] Materialized access cache builds at startup and persists with debouncing
- [ ] `label` op uses O(1) cache lookup, not per-file CouchDB query
- [ ] `access_age` op uses O(1) cache lookup with `missing` parameter support
- [ ] `replicated` op queries replica documents correctly
- [ ] `readdir` queries by `source.export_parent` prefix using the index
- [ ] `prefix_replace` and `flatten` produce correct filenames
- [ ] Both conflict policies work; conservative policy wins across mismatched mounts
- [ ] `enforce_steps_on_children` propagates steps to child directories
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

Implement inode lookup from the local CouchDB replica. Verify stability across restarts. Inode space: 0 invalid, 1 root, 2–999 reserved, 1000+ randomly assigned at file creation.

### 4.4 — Local File Access (Tier 1)

Implement `open` and `read` for files on this node. Verify the file exists at `source.export_path`; return `ENOENT` if stale. Validate that the canonicalized path is under a configured watch path (export_path containment check). Record access in the materialized access cache.

### 4.5 — Full-File Cache

Implement the cache at `/var/lib/mosaicfs/cache/`. Create SQLite `index.db` with schema: `cache_key` (file_uuid), `file_id`, `mtime`, `size_on_record`, `block_size`, `block_map`, `cached_bytes`, `last_access`, `source`. Shard prefix = first 2 UUID chars. Downloads go to `cache/tmp/`, atomic rename on completion. Staleness check: compare `mtime` and `size` against file document. Full-file mode for files below the size threshold (default 50 MB).

### 4.6 — Block-Mode Cache

Implement block mode for large files (video/audio streaming). Block map as a sorted `Vec<Range<u64>>` of present intervals, serialized as binary blob in SQLite. Implement: presence check (binary search), missing range calculation, interval insert with merge. Sparse file writes. Coalesce adjacent missing sub-ranges before issuing HTTP range requests. Fragmentation guard: promote to full-file download if intervals exceed 1,000.

Write unit tests for all block map operations.

### 4.7 — Remote File Fetch (Tier 4)

Implement the transfer discovery sequence: file doc → node doc → `transfer.endpoint` → HMAC-signed request. Full-file mode: stream to staging, verify `Digest` trailer (SHA-256), move to final location. Block mode: `Range` request, write to sparse file, update block map in a single SQLite transaction. Implement download deduplication via `Shared` futures keyed by `(file_id, block_range)`.

### 4.8 — Network Mount Tiers (2 and 3)

Implement Tier 2 (CIFS/NFS): check node document for `network_mounts` entry covering the file, translate path, open locally. Implement Tier 3 (iCloud/Google Drive local sync): same check for `icloud_local`/`gdrive_local` mount types; add iCloud eviction detection via extended attribute, fall through to Tier 4 if evicted.

### 4.9 — Cache Eviction

LRU eviction using `cached_bytes` and `last_access` in `index.db`. After each cache write, check total size against cap (default 10 GB) and free space minimum (default 1 GB). Evict full entries (including partial block-mode entries) in ascending `last_access` order.

### 4.10 — Filesystem Watcher

Implement the `notify`-based watcher. Start after initial crawl. Debounce events over 500ms per path. Correlate renames into a single update. Event storm throttling: switch to full crawl if events exceed 1,000/sec for 5 seconds.

### 4.11 — Reconciliation After Reconnect

Detect reconnection via CouchDB replication state. Run expedited full crawl (mtime/size fast-path) before resuming watch mode.

### Phase 4 Checklist

- [ ] `mosaicfs-vfs` crate exists; readdir evaluator moved into it
- [ ] FUSE mount works, `ls /mnt/mosaicfs` returns results
- [ ] `getattr` returns correct metadata
- [ ] Inodes stable across restarts
- [ ] Tier 1 local file access works with containment check
- [ ] Access tracking fires on VFS open
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

Initialize React + Vite inside the `mosaicfs-server` static directory. Install shadcn/ui, TanStack Query, PouchDB. Configure Vite to proxy API calls in development. Set up routes for all nine pages.

### 5.2 — Authentication Shell

Login page, auth context. On login, receive JWT + CouchDB session token for `mosaicfs_browser`. Hold both in memory only (never localStorage/cookies). Auth guard on all routes.

### 5.3 — PouchDB Sync

Configure pull-only PouchDB replication using the `mosaicfs_browser` session token. PouchDB becomes the source of truth for document-level data; TanStack Query reads from PouchDB. Direct API calls reserved for mutations and non-document endpoints. Push attempts rejected at database level.

### 5.4 — Navigation Shell

Sidebar with all pages, top bar with instance name and user menu. Responsive collapse to bottom tabs. Shared components: node badge (friendly name + colored status pill, click to navigate) and label chip (solid for direct, outlined for inherited).

### 5.5 — Dashboard

Node health strip, error banner, search bar with keyboard shortcut, system totals (files, nodes, storage), recent activity feed.

### 5.6 — Nodes Page

List view with status filter. Agent detail: subsystem status, storage topology, utilization trend chart, watch paths, network mounts CRUD, errors (last 50 from agent_status).

### 5.7 — File Browser

Two-panel: lazy-loaded directory tree + sortable contents table. Breadcrumbs, inline filter, file detail drawer (metadata, labels with direct/inherited distinction, inline preview for images/PDF/text, download button). Read-only indicators with tooltips for planned write features.

### 5.8 — Search Page

Debounced search bar, label filter chips (ANDed with query), result list with infinite scroll, file detail drawer reuse.

### 5.9 — Labels Page

Two tabs: Assignments (sortable table from PouchDB, path filter, click to open file drawer) and Rules (table with enable/disable toggle, rule editor drawer with live preview).

### 5.10 — Virtual Filesystem Page

Two-panel: directory tree with mount badges + contents table. Directory CRUD. Mount editor drawer: `enforce_steps_on_children` toggle, mount source cards, step pipeline editor (all 10 ops including `access_age`, `replicated`, and `annotation`), live preview panel calling the preview endpoint with 500ms debounce. Delete confirmation with cascade warning.

### 5.11 — Storage Page

Utilization table with color-coded bars (by percentage), per-node trend charts with date range picker. Data from `utilization_snapshot` documents fetched on-demand via API (not PouchDB).

### 5.12 — Settings Page

Four tabs. Credential management (create with one-time secret display, enable/disable, delete). Storage backends (replication target CRUD: S3, B2, directory, agent — wired in Phase 6). General configuration. About tab with reindex trigger and PouchDB replica size display. Backup/restore controls (stubs, wired in Phase 8).

### Phase 5 Checklist

- [ ] Login, JWT auth, protected routes work
- [ ] PouchDB syncs all permitted document types, live updates visible
- [ ] Node health strip updates on heartbeats
- [ ] File Browser tree lazy-loads, breadcrumbs work
- [ ] File detail drawer shows labels with direct/inherited distinction
- [ ] Label editing works (add, remove direct; inherited shows rule name)
- [ ] File download from remote node works
- [ ] Inline preview renders images, PDF, text
- [ ] Search with label filters works correctly
- [ ] Labels page rule toggle updates immediately
- [ ] Rule editor live preview shows correct file count
- [ ] VFS mount editor handles all 10 step ops
- [ ] Mount live preview updates as configuration changes
- [ ] Inherited ancestor steps shown read-only in child editor
- [ ] Credential create shows secret once
- [ ] Notification bell present (stub)
- [ ] Storage backend cards present (stub)
- [ ] Settings page shows PouchDB replica size

---

## Phase 6 — Replication

**Goal:** Files replicate to external targets based on rules. Tier 4b failover serves replicas when source nodes are offline.

**Milestone:** Configure a replication rule targeting an S3 bucket. Watch files upload during the scheduled window. Take the source node offline. Access the file through the VFS — it serves from the S3 replica.

**Dependencies:** Requires Phase 2 (Control Plane) and Phase 3 (Rule Engine — for step pipeline evaluation of replication rules). Tier 4b failover (6.8) requires Phase 4 (VFS).

### 6.1 — Storage Backend and Replication Rule Documents

Wire up `storage_backend` and `replication_rule` document CRUD endpoints. Storage backend document: `storage_backend::{name}` with type, mode (target), hosting_node_id, and type-specific config. Replication rule document: UUID-based `_id`, step pipeline, target reference.

Note: only `mode: "target"` is implemented in v1. Source-mode backends are out of scope.

### 6.2 — Replica Document

Implement `replica` document (`replica::{file_uuid}::{target_name}`). Status values: `"current"`, `"stale"`, `"frozen"`. These are CouchDB documents written by the agent's replication subsystem, replicated to all nodes for Tier 4b lookup.

### 6.3 — Replication Subsystem Core

Implement the replication subsystem as a core component of the agent (not a plugin). The subsystem runs inside the agent process alongside the crawler and VFS layer, receiving file events directly from the agent's internal event system.

Local SQLite database (`replication.db`) with `replication_state` and `deletion_log` tables (schema in `replication-plugin-design.md` — retained as supplementary reference for implementation details). Event processing: on `file.added`/`modified` whose `file_id` matches enabled replication rules → queue upload. On `file.deleted` → check manifest, apply retention policy.

Rule evaluation uses the step pipeline engine directly (same instance as the VFS layer) — the replication subsystem is in-process and has direct access to the materialized label cache, access cache, and replica index without any IPC indirection.

### 6.4 — Storage Backend Adapters

Implement thin I/O adapters as Rust trait implementations of the `BackendAdapter` trait (defined in `mosaicfs-common`, per `architecture/18-storage-backends.md`):
- **S3**: AWS SDK multipart upload, `ListObjectsV2`, streaming download. Connection pooling. Start here as reference implementation.
- **B2**: S3-compatible API with custom endpoint. Share implementation with S3.
- **Directory**: Atomic write (temp → fsync → rename), walk for listing.
- **Agent**: `POST /api/agent/replicate/{file_id}` on destination agent.

Remote key scheme: `{prefix}/{file_uuid_8}/{filename}`.

### 6.5 — Bandwidth and Scheduling

Schedule windows: queue events outside window, drain FIFO when window opens. Token bucket rate limiter shared across concurrent uploads per target. Batching (up to 100 files). Configurable `workers` field controls parallel uploads per target (default 2).

### 6.6 — Replication Annotations and Status

Write one annotation per replicated file with per-target status (`current`, `stale`, `pending`, `frozen`, `failed`). Batch annotation writes, flush every `flush_interval_s` seconds (default 60). Also write `replica` documents to CouchDB for Tier 4b visibility.

### 6.7 — Rule Re-evaluation

Periodic full scan (configurable, default daily) via the full sync mechanism (`POST /api/nodes/{node_id}/replication/sync`). Detects: files newly matching rules, stale replicas, files that no longer match (un-replication). `access.updated` events trigger real-time re-evaluation for access_age rules.

Un-replication behavior per `remove_unmatched` setting: `false` (default) → freeze; `true` → move to deletion_log with retention.

### 6.8 — Tier 4b Failover

Add Tier 4b to VFS tiered access, evaluated when Tier 4 fails because the owning node is offline:
1. Query local CouchDB for `replica` documents with `status: "current"` or `"frozen"`
2. For `agent` targets: fetch from replica agent's transfer endpoint
3. For `s3`/`b2` targets: invoke the local `BackendAdapter` implementation's download method, write to staging path, cache and serve
4. For `directory` targets: open directly if locally accessible
5. Cache locally and serve

### 6.9 — Restore Operations

Implement restore API: `POST /api/replication/restore` (initiate), `GET .../restore/{job_id}` (progress), `POST .../restore/{job_id}/cancel`, `GET .../restore/history`. Support partial restore with path_prefix and mime_type filters. Restore preserves file identity (UUID-based `_id`).

### 6.10 — Replication State Recovery

Handle lost SQLite database: detect on startup, enter rebuild mode, issue `manifest_rebuild_needed` notification, reconstruct from target listing on next full scan. Handle lost annotations: rebuild from SQLite manifest during periodic flush.

### Phase 6 Checklist

- [ ] Storage backend CRUD endpoints work (target mode only)
- [ ] Replication rule CRUD endpoints work
- [ ] Replication subsystem starts with the agent and processes file events
- [ ] Files matching rules upload to S3 target
- [ ] Schedule windows respected (queued outside, drained inside)
- [ ] Bandwidth limiting enforced via token bucket
- [ ] Replica documents written to CouchDB
- [ ] Replication annotations show per-target status
- [ ] `file.deleted` triggers retention-aware deletion
- [ ] Periodic full scan detects newly matching and un-matched files
- [ ] Un-replication respects `remove_unmatched` setting
- [ ] Tier 4b serves file from replica when source node offline
- [ ] Restore from agent target works (ownership transfer)
- [ ] Restore from S3 target works (download + ownership transfer)
- [ ] Replication state recovery rebuilds manifest from target

---

## Phase 7 — Notification System

**Goal:** System events appear as notification documents and reach the browser in real time via PouchDB.

**Milestone:** Fill a watched volume to trigger a storage warning, see it in the notification bell within seconds, acknowledge it, watch the badge clear.

**Dependencies:** Requires Phase 2. Parallel with Phases 6 and 8.

### 7.1 — Notification Document Type

Notification documents use deterministic `_id` (`notification::{source_id}::{condition_key}`) for deduplication. Lifecycle: active → resolved or acknowledged. Track `first_seen`, `last_seen`, `occurrence_count`. Severity: info, warning, error.

### 7.2 — Agent Notification Writers

Implement notifications for: first crawl complete (info), inotify limit approaching (warning, auto-resolve), cache near capacity (warning, auto-resolve), storage near capacity (warning, auto-resolve), watch path inaccessible (error, auto-resolve), replication error (error), replication target unreachable (error, auto-resolve), replication backlog (warning, auto-resolve), auth timestamp rejected (error), replication manifest rebuild needed (warning).

### 7.3 — Control Plane Notifications

New node registered (info), credential inactive (warning), CouchDB replication stalled (warning), control plane disk low (warning), persistent CouchDB conflicts (warning — from the conflict monitoring background task).

### 7.4 — Notification REST API

`GET /api/notifications` (with status/severity filters), `POST /api/notifications/{id}/acknowledge`, `POST /api/notifications/acknowledge-all`, `GET /api/notifications/history`.

### 7.5 — Web UI Notification Bell

Bell icon in top nav with unread count badge (red for errors, amber for warnings). Slide-in notification panel: severity-grouped, action buttons (with API endpoints), acknowledge controls, history link. Dashboard alert banner for active errors. Live updates via PouchDB changes feed.

### Phase 7 Checklist

- [ ] Agent writes notifications to CouchDB on relevant events
- [ ] Deterministic _id prevents duplicate accumulation
- [ ] Control plane writes system-level notifications
- [ ] Notifications replicate to browser via PouchDB
- [ ] Bell icon shows correct unread count with severity coloring
- [ ] Notification panel renders with action buttons and updates live
- [ ] Acknowledge updates document status
- [ ] Dashboard alert banner appears for active errors

---

## Phase 8 — Backup and Restore

**Goal:** Download minimal or full backups as JSON; restore into a fresh instance.

**Milestone:** Take a minimal backup, destroy the Compose stack, recreate it, restore the backup, see virtual directories and replication configs reappear. Agents reconnect and re-crawl.

**Dependencies:** Requires Phase 2 (API) and Phase 5 (UI). Independent of notifications.

### 8.1 — Backup Generation

`GET /api/system/backup?type=minimal` — essential documents only (virtual directories, label assignments & rules, credentials, storage backends, replication rules, partial node documents with network_mounts only). Excludes: file documents, utilization snapshots, annotations, operational history. Size: typically <10 MB.

`GET /api/system/backup?type=full` — complete CouchDB database snapshot. Both stream as `Content-Disposition: attachment` JSON in `_bulk_docs` format.

### 8.2 — Restore Process

`POST /api/system/restore` — validate JSON, check all documents have recognized `type` fields, bulk write. Only permitted into an empty database (check document count, reject otherwise). For minimal backups: extract `network_mounts` from partial node documents, merge via PATCH. Return `{ restored_count, errors }`.

### 8.3 — Developer Mode

`--developer-mode` flag on control plane (default off). Enables `DELETE /api/system/data` for database wipes during development.

### 8.4 — Web UI Backup Controls

Settings → About: download buttons (minimal/full). Restore section visible only when database is empty. Post-restore banner: "Restart all agents."

### Phase 8 Checklist

- [ ] Minimal backup contains essential documents only (<10 MB typical)
- [ ] Full backup contains complete database
- [ ] Restore only permitted into empty database
- [ ] Network mounts merged correctly for minimal restore
- [ ] DELETE endpoint requires developer mode flag
- [ ] Settings page backup/restore controls work
- [ ] Post-restore, agents reconnect and re-crawl

---

## Phase 9 — CLI and Desktop App

**Goal:** `mosaicfs-cli` covers common management tasks. The Tauri desktop app wraps the web UI with native integration.

**Milestone:** `mosaicfs-cli files fetch /documents/report.pdf --output ~/Downloads/` downloads a file. The desktop app can drag a file to Finder.

### 9.1 — CLI Foundation

Create `mosaicfs-cli` in the workspace. Load config from `~/.config/mosaicfs/cli.toml`. JWT authentication with in-memory caching. `clap` for argument parsing. Default human-readable output; `--json` for scripting. `--quiet` and `--verbose` flags.

### 9.2 — CLI Commands

```
mosaicfs-cli nodes list | status <node-id>
mosaicfs-cli files search <query> | stat <file-id> | fetch <file-id> [--output <path>]
mosaicfs-cli vfs ls | tree | mkdir | rmdir | show | edit <virtual-path>
mosaicfs-cli storage overview | history <node-id> [--days 30]
mosaicfs-cli credentials create --name <name> | list | revoke <key-id>
mosaicfs-cli system health | reindex | backup [--type minimal|full] | restore <file>
mosaicfs-cli replication status | restore <target> [--source-node <id>] [--dest-node <id>]
```

### 9.3 — Tauri Desktop App

Wrap the React frontend in Tauri. Native additions: persistent window state, system tray, native file dialogs, drag-to-Finder. Read-only in v1. Target macOS and Linux.

### Phase 9 Checklist

- [ ] CLI authenticates and maintains JWT
- [ ] All commands work with human and JSON output
- [ ] `files fetch` downloads with progress indication
- [ ] `replication status` and `replication restore` commands work
- [ ] Tauri builds on macOS and Linux
- [ ] System tray and drag-to-Finder work on macOS

---

## Phase 10 — Hardening and Production Readiness

**Goal:** Graceful failure handling, automatic recovery from transient errors, acceptable performance at target scale, actionable observability.

This phase runs continuously from Phase 2 onward, not as a final gate.

### 10.1 — Error Classification and Retry

Implement the standardized retry parameters: 1s initial delay, 2x multiplier, 60s cap, ±25% jitter. Apply the per-context retry table (HTTP transfer, replication uploads, heartbeat, CouchDB replication reconnect).

### 10.2 — Structured Logging

`tracing` with consistent fields: `node_id`, `file_id`, `operation`, `duration_ms`, `error`. INFO in production, runtime-adjustable. 50 MB rotation, 5 files retained (250 MB total). Stderr in development, file in production.

### 10.3 — Health Checks and Stale Detection

Wire `GET /health` endpoints to real subsystem data (pouchdb, replication, vfs_mount, watcher, transfer_server). Control plane polls agents every 30 seconds; mark offline after 3 missed checks (90s). On control plane restart, re-poll all nodes. Conflict monitoring background task (60s interval), notification if conflict persists >5 minutes.

### 10.4 — inotify Limit Handling

Graceful degradation: unwatched directories fall back to nightly crawl. Log warnings near the limit. `agent init` sets `fs.inotify.max_user_watches = 524288` on Linux.

### 10.5 — Large File Handling

Verify VFS reads, cache writes, and transfer streaming don't buffer full files in memory. Verify `Digest` trailer computation is streaming. Verify replication subsystem streams uploads without full buffering.

### 10.6 — Replication Edge Cases

Test: control plane unreachable at startup (queue and retry), reconnect after extended outage (reconciliation crawl), clock skew (log warning if >2 minutes).

### 10.7 — Scale Testing

Seed 500K file documents (target scale across 20 nodes). Measure: initial crawl time (100K files on disk), readdir latency (10 mount sources), replication cold-start sync, search latency, cache eviction throughput, label cache memory, access cache memory. For block cache: 10 GB video, random seeks, verify interval count stays under 20 after realistic viewing.

### 10.8 — Installer Polish

Clean `agent init` prompts, URL validation, success confirmation with mount path. README with prerequisites, control plane setup, and agent installation per platform.

### Phase 10 Checklist

- [ ] Transient errors retry with standardized backoff; permanent errors surface to UI
- [ ] Structured logs have consistent fields
- [ ] Health polling marks offline nodes within 90 seconds
- [ ] inotify exhaustion degrades gracefully
- [ ] Large files stream without full buffering
- [ ] Agent starts correctly when control plane is unreachable
- [ ] Reconciliation runs after extended outage
- [ ] 500K-file scale test passes with acceptable performance
- [ ] Label and access caches fit within memory budget at scale
- [ ] `agent init` works end-to-end on macOS and Linux

---

## Testing Strategy

**Unit tests** — test the hard invariants with `#[test]`: document serialization round-trips, step pipeline evaluation (all 10 ops), cache key computation, HMAC signatures, block map interval operations, label cache incremental updates, access cache debounce logic, replica document lifecycle.

**Integration tests** — require a real CouchDB via Docker Compose:
- Replication filter correctness: write documents, replicate, verify only expected documents arrive per flow
- Backup/restore round-trip: backup, wipe, restore, verify fidelity
- Transfer server: two agents, fetch a file peer-to-peer, verify bytes match and `Digest` trailer
- Replication subsystem: mock S3 target, verify upload/download/delete lifecycle
- Tier 4b failover: take source offline, verify replica served

**Development environment:**
- Development happens inside of a devcontainer named "dev" defined in .devcontainer/docker-compose.yml.
- The development CouchDB database is accessible using three environment variables: COUCHDB_URL, COUCHDB_USER, COUCHDB_PASSWORD
- Local agent configured with `watch_paths` pointing to a test directory
- `scripts/seed-test-data.sh` creates sample files, virtual directories, labels, storage backends, and replication rules
- `--developer-mode` flag enables database wipe between test cycles

**Performance benchmarks** — Phase 10 seeds 500K file documents and measures crawl time, readdir latency, replication sync, search latency, and cache throughput.

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
| iCloud backend unreliable (no official API) | High | Low | Tier 3 iCloud eviction detection is best-effort; files fall through to Tier 4. Documented limitation. |
| inotify watch exhaustion | High | Medium | Graceful degradation to nightly crawl. Installer raises system limit. |
| PouchDB browser replica too large | Low | Medium | Settings page shows size. Warning at 500 MB. Server-side pagination is the future fix. |
| FUSE bindings (`fuser`) lacking features | Low | High | Evaluate API surface before Phase 4. Fallbacks: `fuse-rs` or direct binding. |
| Replication SQLite loss | Low | Medium | Automatic rebuild from target listing. Degraded mode notifications. |
| Replica staleness during extended source outage | Medium | Medium | `"frozen"` status clearly communicated in UI. Restore operations documented. |

---

## Out of Scope for v1

These features are referenced in the architecture but not implemented in v1:

- **VFS write support** — read-only filesystem. Largest v2 item.
- **Plugin system** — executable and socket plugins, annotation writes, `dispatch_rules`, query routing, `dashboard_widget` capability. The `plugin` and `annotation` document schemas are defined but no v1 component reads or writes them.
- **Source-mode storage backends** — S3, B2, Google Drive, OneDrive, and iCloud as file sources. Depends on plugin infrastructure (`crawl_requested`, `materialize` events). The `storage_backend` document type is used in v1 for replication targets only.
- **Bridge nodes** — `role: "bridge"` node type and `provides_filesystem` plugins. Depends on plugin infrastructure.
- **Tier 5 materialize** — on-demand file materialization via plugins. Depends on plugin infrastructure.
- **Federation** — schema accommodations in place (peering_agreement, federated_import documents), no implementation.
- **Full-text content search** — filename only. Content indexing needs a search engine (Tantivy or Meilisearch via plugin).
- **Multi-user support** — single user per instance. Path to multi-user is federation.
- **File mutation API** — REST API is read-only for files.
- **Metadata search filters** — search accepts only a query string and label filter.
- **Windows agent** — macOS and Linux only.
- **macOS File Provider / Windows CFAPI** — FUSE only for v1.
- **GIO/KIO desktop integration** — FUSE only for v1.
- **Tauri mobile** — PWA covers mobile.
- **Automatic scheduled backups** — on-demand only; scriptable via `curl` or CLI.
- **JWT key rotation** — signing key stable for deployment lifetime.
- **Plugin sandboxing** — deferred with the rest of the plugin system.

---

*MosaicFS Implementation Plan v0.5*
