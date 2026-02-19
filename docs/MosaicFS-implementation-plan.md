# MosaicFS — Implementation Plan

*v0.1 Draft*

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
- [Phase 6 — Cloud Bridges](#phase-6--cloud-bridges)
- [Phase 7 — CLI and Desktop App](#phase-7--cli-and-desktop-app)
- [Phase 8 — Hardening and Production Readiness](#phase-8--hardening-and-production-readiness)
- [Risk Register](#risk-register)
- [What is Explicitly Out of Scope for v1](#what-is-explicitly-out-of-scope-for-v1)

---

## Overview

This document describes the recommended build order for MosaicFS v1. Each phase ends with a concrete, testable milestone — something the system can actually do that it could not do before. Phases are sized for solo part-time development. Later phases depend on earlier ones; skipping ahead is possible but makes debugging harder.

The architecture document is the authoritative reference for all design decisions, schemas, and API contracts. This plan references it but does not repeat it. When this document and the architecture document conflict, the architecture document wins.

---

## Guiding Principles

**Build end-to-end slices, not horizontal layers.** Completing the full path from "agent crawls a file" to "that file appears in the database" in Phase 1 is more valuable than completing the entire database schema before writing any agent code. Thin vertical slices catch integration problems early.

**Make each phase observable.** Every phase should include enough logging and introspection that you can see what the system is doing without attaching a debugger. A phase that produces correct output but silent output is harder to debug in the next phase.

**Test the hard invariants, not the plumbing.** Don't write tests for getters and setters. Do write tests for: inode stability across restarts, transfer `Digest` trailer verification, rule engine evaluation order, and VFS read consistency. These are the properties that will cause subtle production bugs if they break.

**Defer write complexity.** The VFS layer is read-only in v1. The rule engine evaluates on demand without writing to file documents. The API has no bulk mutation endpoints beyond agent file upserts. Write paths are the hardest part of a distributed filesystem — deferring them is correct, not lazy.

**One working cloud bridge before adding more.** S3 is the simplest bridge to implement. Get it fully working — polling, indexing, VFS access, error handling — before starting Google Drive. A working reference implementation makes each subsequent bridge faster to build.

---

## Dependency Map

```
Phase 1: Foundation
  └─► Phase 2: Control Plane & API
        └─► Phase 3: Rule Engine
              ├─► Phase 4: VFS
              └─► Phase 5: Web UI
                    └─► Phase 7: CLI & Desktop
        └─► Phase 6: Cloud Bridges
              └─► (feeds Phase 4 and Phase 5)
  └─► Phase 8: Hardening (runs in parallel with later phases)
```

Phase 8 is not a final gate — hardening work should begin as soon as Phase 2 is complete and continue in parallel with later phases.

---

## Phase 1 — Foundation

**Goal:** An agent binary that crawls a configured directory, writes file documents to a local CouchDB instance, and replicates them to a control plane CouchDB instance. No API, no VFS mount, no rules. Just files in the database.

**Milestone:** Run the agent on your laptop, configure it to watch `~/Documents`, and see file documents appear in Fauxton (the CouchDB web UI) on both the local and control plane instances.

### 1.1 — Repository and Project Structure

Set up the Cargo workspace with three crates: `mosaicfs-agent`, `mosaicfs-server`, and `mosaicfs-common`. The `common` crate holds document type definitions, serialization, and shared utilities. Starting with the workspace structure prevents painful refactoring later when types need to be shared between crates.

### 1.2 — Docker Compose Stack

Write the initial `docker-compose.yml` for the control plane. At this stage it contains only CouchDB. Configure CouchDB with an admin user, bind to localhost only, and create the `mosaicfs` database. Add a `healthcheck` so dependent services wait for CouchDB to be ready. Document the setup steps in a `DEVELOPMENT.md` file.

### 1.3 — CouchDB Document Types

Define Rust structs for all eight v1 document types in `mosaicfs-common`, with `serde` serialization and deserialization. Pay attention to the `_id` format conventions from the architecture document — they are load-bearing. Include unit tests that round-trip each document type through JSON serialization to catch field name mismatches early.

### 1.4 — Agent Configuration

Implement `agent.toml` parsing using `serde` and `toml`. Required fields for Phase 1: `control_plane_url`, `node_id` (read from `node_id` file or generated on first run), `watch_paths` array, `access_key_id`, and `secret_key`. Validate configuration at startup and exit with a clear error message if required fields are missing. The `node_id` file is created on first run and never changes — this is the node's stable identity.

### 1.5 — Local CouchDB Client

Implement a CouchDB HTTP client in the agent. Required operations for Phase 1: `get_document`, `put_document`, `bulk_docs`. Use `reqwest` for HTTP. This client will grow throughout the implementation; keep the interface clean so it is easy to extend. Do not use a third-party CouchDB crate — the interface is simple enough that a hand-rolled client gives you full control over error handling and request formatting.

### 1.6 — Filesystem Crawler

Implement the initial crawl logic. Walk all configured `watch_paths` using `walkdir`. For each file: stat the path and check whether `(export_path, size, mtime)` matches the existing document — if so, the file is unchanged and the document is skipped. Write new or changed file documents immediately. No content hashing is performed — change detection relies entirely on `size` and `mtime`.

Batch writes using `_bulk_docs` in groups of 200 documents. Log a summary at completion: files found, files changed (vs skipped), documents written, duration.

### 1.7 — CouchDB Replication Setup

Configure bidirectional continuous replication between the agent's local CouchDB and the control plane CouchDB. Use the replication filters from the architecture document (Flow 1 and Flow 2). At this stage the filters can be simplified — push everything the agent produces, pull everything from the control plane. The proper filtered replication will be tightened in Phase 2 when credentials and rules exist.

Implement reconnection logic: monitor the replication status endpoint and restart replication if it stops. Log replication state changes.

### 1.8 — Node Document

On startup, the agent writes or updates its `node` document: `friendly_name` (defaults to hostname), `platform`, `status: "online"`, `last_heartbeat`. The heartbeat update runs on a 30-second timer for the rest of the agent's lifetime. On clean shutdown, set `status: "offline"`.

### Phase 1 Completion Checklist

- [ ] Agent starts, creates `node_id` file on first run
- [ ] Agent crawls configured paths and writes `file` documents
- [ ] Stat fast-path skips unchanged files on repeated crawls (size + mtime match = no document write)
- [ ] Documents replicate to control plane CouchDB
- [ ] Node document with heartbeat appears in both databases
- [ ] Agent exits cleanly on SIGTERM with status set to offline

---

## Phase 2 — Control Plane and REST API

**Goal:** The Axum API server is running, all REST endpoints exist (even if some return stub data), HMAC and JWT authentication work, and `curl` can retrieve indexed files from Phase 1.

**Milestone:** Run `curl -H "Authorization: Bearer <token>" https://localhost:8443/api/files` and receive a paginated list of the files indexed in Phase 1.

### 2.1 — Axum Server Skeleton

Set up the `mosaicfs-server` binary with Axum. Configure TLS using a self-signed certificate generated at first run (store in `certs/`). Register all API routes from the architecture document as stubs that return `501 Not Implemented`. This gives you a complete route map to fill in. Add request logging middleware: method, path, status code, duration.

### 2.2 — Credential Management

Implement the `credential` document type operations: create (generate access key ID and secret, hash secret with Argon2id, store document), list, get, enable/disable, delete. The secret key is returned exactly once — in the response to the create request — and is never stored in recoverable form.

### 2.3 — JWT Authentication

Implement `POST /api/auth/login`: accept `{access_key_id, secret_key}`, verify against the stored Argon2id hash, issue a 24-hour JWT signed with a server-side secret. Implement the JWT middleware that validates Bearer tokens on all non-agent endpoints. Implement `GET /api/auth/whoami` and `POST /api/auth/logout`.

### 2.4 — HMAC Authentication

Implement the HMAC-SHA256 request signing middleware for agent endpoints under `/api/agent/`. Validate the canonical signed string (method + path + timestamp + body hash), reject requests with timestamps older than 5 minutes, and look up the credential by access key ID. This middleware will be used by all agent-internal endpoints.

### 2.5 — Node Endpoints

Implement the full `/api/nodes` endpoint group: list, get, register (called by agent init), patch (friendly name, watch paths), delete (soft disable). Implement `/api/nodes/{node_id}/status` returning the agent's `agent_status` document. Implement `/api/nodes/{node_id}/mounts` CRUD (reads and writes the `network_mounts` array on the node document).

### 2.6 — File and Search Endpoints

Implement `GET /api/files`, `GET /api/files/{file_id}`, and `GET /api/files/by-path`. Implement `GET /api/search?q=...` with substring and glob matching against the `name` field. These are read-only endpoints backed directly by CouchDB Mango queries. Verify that the indexes defined in the architecture document are created on startup and that queries use them.

### 2.7 — Virtual Filesystem Endpoints

Implement `GET /api/vfs?path=...` and `GET /api/vfs/tree`. At this stage, these query the `virtual_directory` document for the given path and return child directories and any files whose `source.export_parent` matches a mount source. With no directories created yet, they return empty results. Add `POST /api/vfs/directories`, `GET /api/vfs/directories/{path}`, `PATCH /api/vfs/directories/{path}`, and `DELETE /api/vfs/directories/{path}?force=`. Validate on write: `virtual_path` format, no `/federation/` prefix, `system: true` directories cannot be deleted.

### 2.8 — Agent Internal Endpoints

Implement the `/api/agent/` endpoint group: `POST /api/agent/heartbeat`, `POST /api/agent/files/bulk` (with per-document success/error response), `POST /api/agent/status`, `POST /api/agent/utilization`, `GET /api/agent/credentials`, `GET /api/agent/transfer/{file_id}`.

The bulk file upsert endpoint is the most important of these. It must handle partial failures gracefully — a single malformed document in a batch must not cause the entire batch to fail. Test with batches that include both valid and invalid documents.

### 2.9 — Agent Init Command

Implement `mosaicfs-agent init`: prompt for control plane URL and access key credentials (secret read from stdin with echo disabled), register the node via `POST /api/nodes`, write `agent.toml`, install the systemd unit file (or launchd plist on macOS), and start the service. This is the primary user-facing setup flow.

### 2.10 — File Content Delivery

Implement `GET /api/files/{file_id}/content`. The control plane looks up the file document, determines the owning node, and proxies the bytes. For Phase 2, implement only the local file case (the file is on the same machine as the control plane) and the remote agent HTTP fetch (Tier 4 of tiered access). The CIFS/NFS and local cloud sync tiers are added in Phase 4 when the VFS layer exists.

Support `Range` headers for partial content delivery. Set `Content-Disposition: attachment; filename="..."` for browser downloads.

### 2.11 — Labels API

Implement the full `/api/labels` endpoint group: `GET /api/labels` (all distinct label values), `GET`/`PUT`/`DELETE` for assignments, `GET`/`POST`/`PATCH`/`DELETE` for rules, and `GET /api/labels/effective` for computing a file's effective label set.

Implement the `label_assignment` document operations: deterministic `_id` from `sha256(export_path)`, upsert semantics on `PUT` (create or replace), validation that `labels` is a non-empty array of non-empty strings.

Implement the `label_rule` document operations: validate that `path_prefix` ends with `/`, that `node_id` is a known node ID or `"*"`, and that `labels` is non-empty. Implement the `GET /api/labels/effective` join: fetch the assignment for the given file (if any), fetch all enabled rules whose `node_id` matches and whose `path_prefix` is a prefix of the given path, and return the union.

Extend `GET /api/search` to support `?label=` parameters. When one or more labels are provided, the search must compute effective labels for each candidate file and filter to those whose effective set contains all requested labels. Note that this requires joining against assignment and rule documents at search time — at home-deployment scale this is acceptable, but document it as a known performance consideration if the file count grows into the hundreds of thousands.

### Phase 2 Completion Checklist

- [ ] Axum server starts with TLS, all routes registered
- [ ] JWT login, whoami, and logout work
- [ ] HMAC authentication validates and rejects requests correctly
- [ ] Node registration, listing, and detail endpoints work
- [ ] File listing and search return Phase 1 indexed files
- [ ] Agent bulk upsert handles partial failures correctly
- [ ] File content downloads work for local and remote-agent files
- [ ] Label assignment CRUD works; assignments survive file re-indexing
- [ ] Label rule CRUD works; `GET /api/labels/effective` returns correct union
- [ ] `GET /api/search?label=` filters by label correctly
- [ ] All endpoints return correct error envelopes on bad input

---

## Phase 3 — Rule Evaluation Engine

**Goal:** Virtual directories can be created, configured with mount sources and filter steps, and the VFS and API `readdir` calls return files matching those mounts. Files can appear in multiple directories simultaneously.

**Milestone:** Create a `/documents/work` directory, add a mount source pointing at `~/Documents` on the laptop node with a glob step for `*.pdf`, and verify that `GET /api/vfs?path=/documents/work` returns the expected PDF files.

### 3.1 — Virtual Directory Document Seeding

Create the root virtual directory document (`_id: "dir::root"`, `virtual_path: "/"`, `system: true`, `mounts: []`) at control plane startup if it does not already exist. Verify that `GET /api/vfs?path=/` returns an empty listing with no errors.

### 3.2 — Step Pipeline Evaluator

Implement the step pipeline evaluation function in `mosaicfs-common` so it can be shared between the VFS layer and the control plane API preview endpoint. The function takes a mount entry, an inherited step chain (from ancestor directories), and a file document, and returns `true` (include) or `false` (exclude).

Write thorough unit tests for the evaluator:
- Each op type with and without `invert`
- `on_match: "include"` and `on_match: "exclude"` short-circuit behavior
- `default_result` fallback
- Unknown op types treated as non-match and continuing
- Empty steps array (all files pass)
- Inherited ancestor steps prepended before mount steps
- Ancestor `exclude` cannot be overridden by child `include`
- The `label` op: match when file has all required labels; no match when any required label is absent; inherited labels from rules are included in the match

The `label` op requires resolving the file's effective label set (direct assignment + matching rules) before evaluating the step. Implement `resolve_effective_labels(file_doc, local_db)` as a function in `mosaicfs-vfs` that performs the two-query join described in the architecture document. Cache the result within a single readdir evaluation pass to avoid re-fetching the same file's labels multiple times if it appears in multiple mount sources.

### 3.3 — Readdir Evaluation

Implement the core `readdir` evaluation logic in `mosaicfs-common`. Given a virtual directory document, it must:

1. Walk the ancestor chain and collect inherited steps from directories with `enforce_steps_on_children: true`
2. For each mount in the directory's `mounts` array, query file documents by `(source.node_id, source.export_parent prefix)` using the CouchDB index
3. Run the combined step pipeline (inherited + mount steps) against each candidate file
4. Apply the mapping strategy (`prefix_replace` or `flatten`) to derive each file's name within the directory
5. Apply `conflict_policy` when two sources produce the same name
6. Also include child `virtual_directory` documents as subdirectory entries

Return the combined listing. Write unit tests that cover: multi-source merging, conflict resolution for both policies, flatten vs prefix_replace, inherited steps filtering out files that mount steps would include, files appearing in two different directories from the same source.

### 3.4 — On-Demand VFS Endpoint

Wire `readdir` evaluation into `GET /api/vfs?path=...`. The endpoint loads the virtual directory document, runs evaluation, and returns the listing. Implement `GET /api/vfs/tree` using recursive evaluation up to the requested depth.

Test with a directory that has two mount sources from different nodes pointing to overlapping paths. Verify both sources contribute files and conflict resolution behaves correctly.

### 3.5 — Directory Preview Endpoint

Implement `POST /api/vfs/directories/{path}/preview`. The request body may contain a draft `mounts` array not yet saved — preview runs evaluation against the submitted configuration rather than the stored document. This is the most important endpoint for the web UI mount editor.

Performance note: at home-deployment scale, a full scan is acceptable. If it becomes slow, add a configurable result limit with a "showing first N matches" indicator.

### 3.6 — Readdir Cache

Implement the short-lived results cache on the `readdir` evaluator (default 5-second TTL per directory). Cache the output keyed by `(virtual_path, directory_document_revision)`. Invalidate on document change via the PouchDB/CouchDB live changes feed. This prevents re-evaluating mount sources on every `lookup` during rapid directory traversal.

### Phase 3 Completion Checklist

- [ ] Root directory document created at startup
- [ ] Step pipeline evaluator passes all unit tests including ancestor inheritance
- [ ] `label` op resolves effective label set correctly (direct assignment ∪ matching rules)
- [ ] `label` op result is cached per-file within a single readdir pass
- [ ] `readdir` evaluation queries by `source.export_parent` prefix using the index
- [ ] `prefix_replace` and `flatten` strategies produce correct filenames
- [ ] Conflict resolution works for both `last_write_wins` and `suffix_node_id`
- [ ] A file in two directories shows the same inode in both listings
- [ ] `GET /api/vfs?path=...` returns files matching the directory's mounts
- [ ] `GET /api/vfs/tree` recurses correctly up to max depth
- [ ] Preview endpoint evaluates unsaved draft mounts correctly
- [ ] Readdir cache invalidates when directory document changes
- [ ] Deleted files (`status: "deleted"`) never appear in listings

---

## Phase 4 — Virtual Filesystem Layer

**Goal:** The virtual filesystem is mountable via FUSE (v1 backend). `ls`, `find`, `cat`, and `cp` all work on files from local and remote agents. The path-based file cache populates on first access and is served from cache on subsequent reads. The common VFS crate (`mosaicfs-vfs`) is established so that future OS-specific backends (macOS File Provider, Windows CFAPI) can be added without touching the core logic.

**Milestone:** Mount the FUSE filesystem, run `ls /mnt/mosaicfs/documents`, open a PDF from a remote agent node in a PDF viewer, and confirm the file is served from cache on the second open.

### 4.1 — VFS Common Crate Skeleton

Create `mosaicfs-vfs` as a separate crate. Define the trait that OS-specific backends implement: at minimum `readdir`, `lookup`, `open`, `read`, and `getattr`. The common crate owns the readdir evaluator (from Phase 3, which should be moved here from `mosaicfs-common`), tiered access logic, and the file cache. OS backends call into the common crate and translate results into their native API calls.

### 4.2 — FUSE Backend Skeleton

Set up the `fuser` integration in the agent binary as the FUSE backend. Implement the minimum required FUSE operations to make the filesystem mountable and browsable: `lookup`, `getattr`, `readdir`. These operations delegate to `mosaicfs-vfs` — no direct CouchDB calls in the backend. An empty mount that can be `ls`-ed is the first sub-milestone.

### 4.3 — Inode Resolution

Implement inode lookup from the CouchDB replica. File documents and virtual directory documents each carry a stable `inode` field. The VFS backend maps between inode numbers and document IDs using the local index. Verify that inodes are stable across agent restarts — the same file must always present the same inode number to the OS.

Implement the inode space partitioning from the architecture document: 0 is invalid, 1 is root, 2–999 reserved, 1000+ randomly assigned.

### 4.4 — File Open and Read (Local Tier)

Implement `open` and `read` for Tier 1: files that physically reside on this node. Look up the file document, verify the file still exists at `source.export_path`, and open it directly. Return `ENOENT` if the file no longer exists at its real path (stale document — the watcher will eventually clean this up).

### 4.5 — File Cache: Full-File Mode

Implement the full-file cache path at `/var/lib/mosaicfs/cache/`. Create the SQLite `index.db` with the schema from the architecture document. Cache keys are derived by SHA-256 hashing `{node_id}::{export_path}`. In-progress downloads go to `cache/tmp/` and are moved atomically to their shard location on completion. Insert a row into `index.db` with `block_size = NULL` and `block_map = NULL` to mark a full-file entry.

Implement the staleness check on cache lookup: compare `mtime` and `size_on_record` against the current file document. A mismatch evicts the entry before fetching fresh.

Full-file mode is used for files below the size threshold (default 50 MB, configurable).

### 4.6 — File Cache: Block Mode

Implement block mode for files above the size threshold. The primary use case is home media — videos and audio where the user opens a file, watches some, seeks, watches more, and never downloads the whole thing.

Implement the block map as a sorted `Vec<Range<u64>>` of present block intervals in memory, serialized as a compact binary blob (pairs of little-endian u64 values) in the `block_map` SQLite column. Implement the three core operations:

- **Is block N present?** Binary search the interval list.
- **Missing blocks in range [A, B)?** Subtract present intervals from the requested range; return gaps as a list of sub-ranges.
- **Mark [A, B) as present:** Insert and merge adjacent/overlapping intervals to maintain the sorted non-overlapping invariant.

Implement sparse file writes: open the cache data file, seek to the correct byte offset for each block, write the fetched bytes. The OS handles the sparseness.

Coalesce adjacent missing sub-ranges before issuing HTTP range requests — if blocks 5, 6, and 7 are all missing, issue one `Range: bytes=5242880-8388607` request rather than three.

Write unit tests for the block map implementation: present/absent queries at boundaries, missing range calculation with partial overlap, interval merging for adjacent and overlapping ranges, and the empty and fully-present edge cases.

### 4.7 — Remote Agent File Fetch (Tier 4)

Implement Tier 4 file access. For full-file mode: issue `GET /api/agent/transfer/{file_id}` (HTTP 200), stream to `cache/tmp/` while accumulating a SHA-256 hash, read the `Digest` trailer, verify, move to final location, update `index.db`. Discard and retry on verification failure.

For block mode: issue `GET /api/agent/transfer/{file_id}` with a `Range` header covering the coalesced missing span, write fetched bytes into the sparse data file at the correct offset, update the block map in `index.db` within a single SQLite transaction.

Implement download deduplication using a `Shared` future keyed by `(file_id, block_range)`. Concurrent reads for the same uncached range share one in-flight request — common when a video player issues parallel read-ahead requests.

### 4.8 — Network Mount Tiers (Tiers 2 and 3)

Implement Tier 2: check the owning node document for a `network_mounts` entry covering the file's `source.export_path`. If found, translate the path to the local mount point and open directly.

Implement Tier 3: same check, but for `mount_type` values of `icloud_local` or `gdrive_local`. Add the iCloud eviction check: test for the `com.apple.ubiquity.icloud-item-evicted` extended attribute before attempting to open. If evicted, fall through to Tier 4.

### 4.9 — Cache Eviction

LRU eviction operates on whole cache entries using the `cached_bytes` and `last_access` columns in `index.db`. After each new cache write, check total `cached_bytes` across all entries against the configured size cap (default 10 GB) and minimum free space constraint (default 1 GB). If either threshold is exceeded, evict entries in ascending `last_access` order until both constraints are satisfied. Remove the sparse data file and `index.db` row together.

### 4.10 — Filesystem Watcher

Implement the incremental file watcher using the `notify` crate. After the initial crawl completes, start watching all configured paths for `CREATE`, `MODIFY`, `DELETE`, and `RENAME` events. Debounce events over a 500ms window per path. Correlate rename events into a single `export_path` update operation. Write updated documents to CouchDB; let replication propagate the change to the control plane.

### 4.11 — Reconciliation After Reconnect

Implement reconnect detection by monitoring the CouchDB replication state. When replication reconnects after an outage, run an expedited full crawl of all watched paths (using the mtime/size fast-path) before resuming normal watch mode. Log the number of changes found during reconciliation.

### Phase 4 Completion Checklist

- [ ] `mosaicfs-vfs` crate exists; readdir evaluator moved from `mosaicfs-common` into it
- [ ] FUSE backend mounts successfully, `ls /mnt/mosaicfs` works
- [ ] `getattr` returns correct metadata for files and directories
- [ ] Inodes are stable across agent restarts
- [ ] Full-file cache (< 50 MB) downloads and serves correctly
- [ ] `Digest` trailer verification rejects corrupted full-file downloads
- [ ] Block map unit tests pass: present/absent queries, missing range calculation, interval merging
- [ ] Block mode fetches only the requested range, not the full file
- [ ] Adjacent missing blocks coalesced into single range request
- [ ] Sparse file writes place bytes at correct offsets
- [ ] Second read of a cached block region does not trigger a network request
- [ ] Concurrent reads for the same uncached range share one in-flight fetch
- [ ] Cache eviction respects size cap and free space minimum
- [ ] Network mount tiers (2 and 3) work for CIFS/NFS and local cloud sync
- [ ] Filesystem watcher detects changes within ~1 second
- [ ] Rename events produce a single document update, not delete+create
- [ ] Reconciliation crawl runs correctly after reconnect

---

## Phase 5 — Web UI

**Goal:** All seven pages of the web UI are implemented and connected to real API data. PouchDB live sync provides real-time updates. The Rules editor with live preview works end-to-end.

**Milestone:** Open the web UI, create a rule using the step editor, watch the live preview populate with matching files, save the rule, navigate to the File Browser, and download a file from a remote node.

### 5.1 — Project Setup

Initialize the React + Vite project inside the `mosaicfs-server` crate's static directory. Install shadcn/ui, TanStack Query, and PouchDB. Configure Vite to proxy API calls to the Axum backend in development. Set up the router (React Router or TanStack Router) with routes for all seven pages.

### 5.2 — Authentication Shell

Implement the login page and the auth context. On login, POST credentials to `/api/auth/login`. The server validates the credentials and returns two tokens: a 24-hour JWT for REST API calls, and a short-lived CouchDB session token for the `mosaicfs_browser` read-only user. Hold both in memory — never written to `localStorage` or cookies. Wrap all authenticated routes in an auth guard that redirects to login if either token is absent or expired.

### 5.3 — PouchDB Sync Setup

Configure PouchDB replication from the control plane CouchDB using the `mosaicfs_browser` session token obtained at login. PouchDB operates in pull-only mode — the `mosaicfs_browser` CouchDB role is read-only, so any attempted push is rejected at the database level rather than relying on filter logic. The PouchDB database becomes the single source of truth for most UI data — TanStack Query reads from PouchDB rather than making direct API calls for document-level data. Direct API calls are reserved for mutations and endpoints without a CouchDB backing (search, file content, rule preview).

### 5.4 — Navigation and Shell

Implement the persistent left sidebar with icons and labels for all eight pages (Dashboard, File Browser, Search, Labels, Nodes, Virtual Filesystem, Storage, Settings). Implement the top bar with instance name and user menu. Implement the responsive collapse to bottom tab bar on narrow viewports. Implement two shared display components used throughout the UI: node badge (small colored pill with friendly name, hoverable for detail) and label chip (pill with label text; solid fill for direct assignments, outlined for inherited labels with a rule-name tooltip on hover).

### 5.5 — Dashboard

Implement the node health strip (one card per node from PouchDB). Implement the error banner (appears when any node is degraded/unhealthy). Implement the quick-access search bar with keyboard shortcut. Implement system totals derived from PouchDB document counts. Implement the recent activity feed from `agent_status` error arrays.

### 5.6 — Nodes Page

Implement the nodes list view with kind filter. Implement the physical agent detail page: status panel, storage topology cards, utilization trend chart (from `GET /api/storage/{node_id}/history`), watch paths, network mounts CRUD, and recent errors table. Implement the cloud bridge detail page: OAuth status, sync controls, storage display.

### 5.7 — File Browser

Implement the two-panel layout: lazy-loaded virtual directory tree (left) and sortable directory contents table (right). Implement breadcrumb navigation. Implement the inline filename filter. Implement the file detail drawer with full metadata display, label section (direct assignment chips with × to remove, inherited chips with rule-name tooltip, inline label input with `GET /api/labels` autocomplete for adding new labels), download button, and inline preview for images, PDFs, and plain text. Implement the right-click context menu on directories including the "Apply labels to this folder and subfolders" shortcut that pre-fills the label rule creation drawer.

### 5.8 — Search Page

Implement the search bar with debounced query submission. Implement the label filter chip row: an "Add label filter" button opens a dropdown populated from `GET /api/labels`; selected labels appear as dismissible chips and are ANDed with the filename query. Implement result display — each result shows name, virtual path, owning node badge, up to 3 effective label chips (with "+N more"), size, and mtime — with infinite scroll pagination. Implement the result interpretation line (filename only, label only, or combined). Reuse the file detail drawer from the File Browser, including full label editing.

### 5.9 — Labels Page

Implement the two-tab layout (Assignments / Rules).

**Assignments tab:** A sortable table of all `label_assignment` documents synced via PouchDB. Columns: node badge, file path, label chips, last updated. A path-substring filter box narrows the list client-side. Clicking any row opens the shared file detail drawer.

**Rules tab:** A table of all `label_rule` documents. Columns: name, node (or "All nodes"), path prefix, label chips, enabled toggle (wired to `PATCH /api/labels/rules/{id}`), created date. An "Add rule" button opens the rule editor drawer. Clicking a row opens the same drawer pre-populated.

**Rule editor drawer:** Node selector (dropdown of known nodes + "All nodes"), path prefix text field with trailing-slash validation, label tag input with `GET /api/labels` autocomplete (new labels can be typed freely), name field, enabled toggle. A live preview panel below the form calls `GET /api/labels/effective` with a sample path and shows the matching file count and up to 10 example filenames; updates with 500ms debounce as the prefix is edited. Save calls `POST` or `PATCH` depending on whether a rule already exists.

### 5.10 — Virtual Filesystem Page

Implement the two-panel layout: collapsible directory tree (left panel) with mount-source count badges, and directory contents table (right panel). Implement the "New folder" button and name/parent-selection dialog. Implement the right-click context menu (Rename, Edit mounts, New subfolder, Delete, Apply labels to folder — the last shortcut navigates to the Labels page with the rule editor pre-filled).

Implement the mount editor drawer: the `enforce_steps_on_children` toggle with inherited-steps read-only display, mount source cards with node/path/strategy fields, per-mount step pipeline editor (op selector; op-specific fields including the `label` op's multi-label tag input; invert toggle; on-match selector; drag-to-reorder; unknown-op passthrough cards), default result toggle, and conflict policy radio buttons.

Implement the live preview panel with 500ms debounce calling `POST /api/vfs/directories/{path}/preview`. This is the most complex component in the UI — build it last within this phase and test thoroughly with mounts that have varied step configurations and inherited ancestor steps.

Implement the delete confirmation dialog with cascade warning for non-empty directories. Disable delete for `system: true` directories with a tooltip.

### 5.11 — Storage Page

Implement the utilization table with color-coded bars and special handling for consumption-billed and iCloud nodes. Implement the per-node trend charts with per-filesystem lines and date range picker.

### 5.12 — Settings Page

Implement the four-tab Settings layout. Implement credential management: table, create modal with one-time secret display, enable/disable, delete with warning. Implement cloud bridge OAuth cards. Implement general configuration fields. Implement the About tab with trigger-reindex button.

### Phase 5 Completion Checklist

- [ ] Login, JWT auth, and protected routes work
- [ ] PouchDB syncs label_assignment and label_rule documents alongside all existing types
- [ ] Node health strip updates when agent heartbeats arrive
- [ ] File Browser tree loads lazily and breadcrumb navigation works
- [ ] File detail drawer shows direct assignment chips and inherited rule chips with correct visual distinction
- [ ] Adding a label from the drawer calls PUT /api/labels/assignments and updates live
- [ ] Removing a direct label chip deletes it; inherited chips show rule name on hover
- [ ] "Apply labels to this folder" right-click shortcut pre-fills the rule editor correctly
- [ ] File detail drawer downloads a file from a remote node
- [ ] Inline preview renders images, PDFs, and text correctly
- [ ] Search label filter chips AND correctly with filename query; result interpretation line reflects combination
- [ ] Search results show up to 3 label chips with "+N more" indicator
- [ ] Labels page Assignments tab lists all label_assignment documents with working path filter
- [ ] Labels page Rules tab enable/disable toggle updates rule immediately
- [ ] Rule editor live preview panel shows correct file count for a given prefix
- [ ] Rule editor saves new rules via POST and edits via PATCH
- [ ] Virtual Filesystem mount step editor handles all seven op types including label
- [ ] label step op's multi-label tag input uses GET /api/labels for autocomplete
- [ ] Mount live preview updates as mounts and steps are edited
- [ ] Inherited ancestor steps shown read-only in child directory editor
- [ ] Delete confirmation warns and cascades correctly
- [ ] Credential create shows secret once and never again
- [ ] OAuth bridge cards present (stubs for now, wired in Phase 6)

---

## Phase 6 — Cloud Bridges

**Goal:** Cloud storage services appear as nodes in MosaicFS. Their files are indexed, appear in the virtual namespace under appropriate rules, and are downloadable through the VFS layer and web UI.

**Milestone:** Configure an S3 bucket as a bridge, create a rule that maps its contents to `/cloud/s3/`, and successfully download a file from S3 through the VFS mount.

### 6.1 — Bridge Runner Infrastructure

Implement the bridge runner framework on the control plane: a common Rust trait with `list(path)`, `stat(path)`, `fetch(path)`, and `refresh_auth()` methods. Implement the polling loop that calls `list()` on a schedule, diffs results against the current index, and writes new or updated `file` documents via `_bulk_docs`. Implement bridge node document management: create on registration, update status and last-sync timestamp after each poll.

### 6.2 — S3 Bridge

Implement the S3 bridge using `aws-sdk-rust`. Each configured bucket registers as a separate bridge node. Simulate directories from key prefixes — S3 has no real directory concept. Handle pagination of `ListObjectsV2` results. Implement `fetch(path)` returning a streaming byte response with a `Digest` trailer.

Implement VFS Tier 4 support for S3: when a file owned by an S3 bridge node is requested, the control plane bridge runner fetches it from S3 and streams it to the requesting agent.

### 6.3 — Backblaze B2 Bridge

Implement the B2 bridge using the S3-compatible API with a custom endpoint. This should be nearly identical to the S3 bridge — share as much implementation as possible. Test with a B2 bucket.

### 6.4 — Google Drive Bridge

Implement the Google Drive bridge using the REST API. Implement OAuth2 with refresh tokens: the `/api/nodes/{node_id}/auth` and `/api/nodes/{node_id}/auth/callback` endpoints are wired to the Google OAuth flow. Store tokens encrypted in `bridges/google_drive/credentials.enc`. Implement delta sync using the Drive Changes API with a stored page token — avoid full listings on every poll.

Wire the OAuth card in the web UI Settings → Cloud Bridges tab.

### 6.5 — Microsoft OneDrive Bridge

Implement the OneDrive bridge using the Microsoft Graph API. OAuth2 flow similar to Google Drive. Maintain a path-to-item-ID mapping since OneDrive uses opaque item IDs internally. Use the Graph delta API for incremental sync.

### 6.6 — iCloud Bridge

Implement iCloud access via the local `~/Library/Mobile Documents/` sync directory on macOS. Register as a bridge node with `bridge_type: "icloud"`. Since there is no API, the bridge runner is simply a periodic crawl of the sync directory — the same logic as a physical agent, but pointed at the iCloud sync path. Implement eviction detection using the `com.apple.ubiquity.icloud-item-evicted` extended attribute. When an evicted file is requested, fall through to Tier 4 — which for iCloud means returning an error, since there is no programmatic way to trigger a download without the system's own eviction logic.

### Phase 6 Completion Checklist

- [ ] Bridge runner trait and polling loop implemented
- [ ] S3 bridge indexes a bucket and files appear in VFS
- [ ] S3 files downloadable through the VFS mount and web UI
- [ ] B2 bridge works identically to S3
- [ ] Google Drive OAuth flow completes in web UI
- [ ] Google Drive delta sync correctly tracks changes
- [ ] OneDrive OAuth flow completes in web UI
- [ ] OneDrive delta sync with item ID mapping works
- [ ] iCloud files indexed from local sync directory
- [ ] iCloud eviction detection falls through correctly

---

## Phase 7 — CLI and Desktop App

**Goal:** `mosaicfs-cli` covers all common management tasks from the terminal. The Tauri desktop app wraps the web UI with native file system integration.

**Milestone:** Use `mosaicfs-cli files fetch /documents/report.pdf --output ~/Downloads/report.pdf` to download a file. Open the desktop app and drag a file from the MosaicFS browser to the macOS Finder.

### 7.1 — CLI Foundation

Create the `mosaicfs-cli` binary in the Cargo workspace. Implement configuration loading from `~/.config/mosaicfs/cli.toml` (control plane URL and access key credentials). Implement the HTTP client with JWT authentication — obtain a token on first request of each session, cache it in memory for the duration of the process.

Use `clap` for argument parsing with subcommand structure. Default output is human-readable table format; `--json` flag outputs raw JSON for scripting. All commands respect `--quiet` (suppress non-essential output) and `--verbose` (show request/response details).

### 7.2 — CLI Commands

Implement all command groups:

```
mosaicfs-cli nodes list
mosaicfs-cli nodes status <node-id>

mosaicfs-cli files search <query>
mosaicfs-cli files stat <file-id>
mosaicfs-cli files fetch <file-id> [--output <path>]

mosaicfs-cli vfs ls <virtual-path>
mosaicfs-cli vfs tree <virtual-path>
mosaicfs-cli vfs mkdir <virtual-path>
mosaicfs-cli vfs rmdir <virtual-path> [--force]
mosaicfs-cli vfs show <virtual-path>
mosaicfs-cli vfs edit <virtual-path>

mosaicfs-cli storage overview
mosaicfs-cli storage history <node-id> [--days 30]

mosaicfs-cli credentials create --name <name>
mosaicfs-cli credentials list
mosaicfs-cli credentials revoke <key-id>

mosaicfs-cli system health
mosaicfs-cli system reindex
```

### 7.3 — Tauri Desktop App

Initialize the Tauri project wrapping the same React frontend used by the web UI. The web UI runs inside the Tauri webview — no duplication of React code. Implement native additions: persistent window state (remember size and position), system tray with quick-access to the app, native file open/save dialogs for the download flow, and drag-to-Finder for files in the file browser (using Tauri's drag-and-drop APIs).

Write operations (move, rename, delete) remain disabled in v1 — the desktop app is a read-only file browser with download capabilities, same as the web UI.

### Phase 7 Completion Checklist

- [ ] CLI authenticates and maintains JWT across commands
- [ ] All CLI commands work with both human and JSON output formats
- [ ] `files fetch` downloads correctly with progress indication
- [ ] Tauri app builds and runs on macOS and Linux
- [ ] System tray icon and quick-access menu work
- [ ] File download via native save dialog works
- [ ] Drag-from-desktop-app to Finder works on macOS

---

## Phase 8 — Hardening and Production Readiness

**Goal:** The system handles failure gracefully, recovers automatically from transient errors, performs acceptably at home-deployment scale, and produces actionable logs and health signals.

This phase runs in parallel with later phases rather than strictly after Phase 7 — begin hardening as soon as Phase 2 is complete.

### 8.1 — Error Classification and Retry

Implement the three-tier error classification from the architecture document: transient errors (retry with exponential backoff, max 60 seconds), permanent errors (log at ERROR level, surface to web UI), soft errors (log but continue). Apply consistently across: CouchDB operations, cloud bridge API calls, agent file fetches, and replication failures.

### 8.2 — Structured Logging

Ensure all components use `tracing` with consistent key-value fields: `node_id`, `file_id`, `rule_id`, `operation`, `duration_ms`, `error`. Production log level is INFO. Runtime-adjustable log level via a signal or API endpoint. Log rotation at 50 MB, keeping 5 rotated files.

### 8.3 — Health Endpoints

Wire `GET /api/health` and `GET /api/health/nodes` to real subsystem data from `agent_status` documents. Implement per-agent health polling: the control plane polls each agent's `GET /health` endpoint every 30 seconds. Mark a node as offline after three consecutive missed checks. Verify the web UI Dashboard reflects status changes within one polling cycle.

### 8.4 — inotify Limit Handling

Implement graceful degradation when the inotify watch limit is exhausted. Directories that cannot be watched are recorded in the agent's `watch_state` document and fall back to coverage by the nightly full crawl. Log a warning when watches are added near the limit. The installer sets `fs.inotify.max_user_watches = 524288` via `/etc/sysctl.d/` — implement this step in the `mosaicfs-agent init` flow on Linux.

### 8.5 — Large File Handling

Test with files larger than the available RAM. Verify that the VFS read path, cache write path, and transfer streaming do not load the full file into memory. Verify that `GET /api/files/{file_id}/content` streams the response and does not buffer the full file in the Axum handler. Verify that `Digest` trailer computation is streaming — the hash is accumulated incrementally, not computed over a buffered copy.

### 8.6 — Replication Edge Cases

Test and handle: control plane unreachable at agent startup (agent should queue local writes and retry); control plane returns after extended outage (reconciliation crawl runs before resuming watch); clock skew between agent and control plane (HMAC timestamp window should tolerate ±5 minutes; log a warning if skew exceeds 2 minutes).

### 8.7 — Scale Testing

Index a realistic dataset: 100,000 files across multiple directories with a range of file sizes. Measure: initial crawl duration, repeated crawl duration (fast-path), VFS `readdir` latency on directories with 1,000+ entries, rule evaluation time for 50 rules against 100,000 files, search response time. Identify and fix any obvious bottlenecks. Document baseline performance figures.

For the block cache specifically, test with a realistic home media scenario: open a 10 GB video file through the VFS mount, seek to several random positions, and measure: time to first frame (latency of first block fetch), seek latency (time from seek to playback resuming), and cache hit rate after a complete watch-with-seeks session. Verify that the interval list stays small (< 20 intervals) after a realistic viewing session and that block map serialization/deserialization is not a bottleneck on the hot path.

### 8.8 — Installer and First-Run Experience

Polish `mosaicfs-agent init`: clear prompts, validation of the control plane URL before proceeding, success confirmation with the VFS mount path. Write a `README.md` covering prerequisites (Docker, Rust toolchain), control plane setup, and agent installation on each platform.

### Phase 8 Completion Checklist

- [ ] Transient errors retry with backoff; permanent errors surface to UI
- [ ] Structured logs have consistent fields across all components
- [ ] Health polling marks offline nodes within 90 seconds
- [ ] inotify exhaustion degrades gracefully without crashing
- [ ] Files larger than available RAM transfer, cache, and stream correctly
- [ ] Agent starts correctly when control plane is unreachable
- [ ] Reconciliation runs correctly after extended control plane outage
- [ ] 100,000-file scale test completes with acceptable performance
- [ ] `mosaicfs-agent init` works end-to-end on macOS and Linux

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| VFS correctness bugs causing data corruption | Medium | High | Read-only v1 eliminates write-path bugs. Test by reading a file through the VFS mount and comparing its size and a spot-check of bytes against a direct read of the source file. |
| `Digest` trailer not supported by some HTTP clients | Low | Low | The trailer is optional — clients that ignore it still get the bytes. Only the agent-to-agent transfer path verifies trailers. Browser downloads and the CLI use the `Content-Disposition` response and trust TLS. |
| CouchDB replication filters behaving unexpectedly | Medium | High | Test filters explicitly with a suite of document fixtures before relying on them in production. Log filter mismatches at WARN level. |
| iCloud bridge unreliable due to no official API | High | Low | iCloud is documented as best-effort. The eviction fallback path is the safety net. Users with critical iCloud files should also configure a physical agent on their Mac. |
| OAuth token refresh failures for Google Drive / OneDrive | Medium | Medium | Implement automatic token refresh with retry. Surface auth expiry prominently in the web UI before it causes sync failures. |
| Large directory trees exhausting inotify watches | High | Medium | Handled in Phase 8 with graceful degradation. The installer raises the system limit. Document the fallback behavior. |
| PouchDB browser sync growing too large over time | Low | Medium | The browser replication filter excludes `utilization_snapshot`. Monitor replica size. Add a purge mechanism if growth becomes a problem. |
| Rust FUSE bindings (`fuser`) lacking necessary functionality | Low | High | Evaluate `fuser` API surface before starting Phase 4. If it lacks needed functionality, `fuse-rs` or a direct FUSE binding are fallbacks. |

---

## What is Explicitly Out of Scope for v1

The following features are designed, documented, or referenced in the architecture document but will not be implemented in v1. They are listed here to prevent scope creep.

**VFS write support.** The virtual filesystem is read-only. No file creation, modification, rename, or deletion through the VFS mount. Write support is the largest single item deferred to v2.

**Federation.** The schema accommodations are in place (`export.peer_ids` on rules, `node_kind: "federated_peer"` reserved, `/federation/` path prefix reserved), but no federation endpoints, peering agreement documents, or cross-instance transfer logic are implemented.

**Full-text content search.** Search is filename and virtual path only. Content indexing requires a separate pipeline and search engine.

**Multi-user support.** Single-user per instance. The path to multi-user is federation, not per-instance access control.

**Write API.** `POST`, `PUT`, `PATCH`, and `DELETE` on files through the REST API. The API is read-only for files in v1.

**Metadata filtering in search.** Filtering search results by file type, size, date range, or owning node. The search endpoint accepts only a query string in v1.

**Windows agent.** The agent targets macOS and Linux. Windows support is possible (the `notify` crate supports `ReadDirectoryChangesW` and FUSE is available via WinFsp) but is not tested or supported in v1.

**Tauri iOS / Android.** The PWA web UI covers mobile. Native Tauri mobile apps are deferred.

**Automatic backup or versioning.** MosaicFS indexes and presents files; it does not copy or version them.

---

*MosaicFS Implementation Plan v0.1 — Subject to revision*
