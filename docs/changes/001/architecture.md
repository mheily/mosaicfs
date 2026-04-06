# Architecture Change 001: Peer Architecture with FileProvider and Loco Settings UI

**Status:** Draft — open items flagged for follow-up

**Sources:** March 17 architecture session, project decisions document.

---

## 1. Current State Summary

### Workspace Crates

| Crate | Type | Key Dependencies | Purpose |
|-------|------|-----------------|---------|
| `mosaicfs-common` | library | serde, chrono, uuid, ts-rs | Shared types and data model |
| `mosaicfs-vfs` | library | fuser, reqwest, rusqlite, notify | VFS logic, CouchDB interaction, FUSE mount |
| `mosaicfs-agent` | binary | axum, walkdir, reqwest | Headless crawler and file indexer |
| `mosaicfs-server` | binary | axum, tower-http, jsonwebtoken, argon2 | REST API (~92 routes), auth, TLS, serves React static assets |

**Excluded from workspace:** `web/src-tauri` (Tauri desktop shell, not part of
the Rust workspace build).

### Frontend

React/TypeScript app (`web/src/`): 13 pages, 24 components, 58 generated types
(via `ts-rs`). Served by `mosaicfs-server` as static assets via
`tower-http::ServeDir`.

### Deployment

Single pod (`deploy/mosaicfs.yaml`) with three containers: **couchdb**,
**mosaicfs-server** (REST API + web UI), **mosaicfs-agent** (crawler). The agent
authenticates to the server using HMAC credentials.

### Code Duplication

Duplicated modules across crates:
- `couchdb.rs` — 3 copies (vfs, agent, server)
- `notifications.rs` — 3 copies (agent, server, server/handlers)
- `replication.rs` — 2 copies (agent, server/handlers)
- `readdir.rs` — 2 copies (vfs, server)

### External Services

- **CouchDB** — metadata store, federation via built-in replication
- **S3-compatible storage** — replication backend for file content

---

## 2. Goal

Restructure MosaicFS as a single-binary peer architecture where every process
accesses CouchDB through shared Rust crates. Replace the React/Tauri frontend
with a minimal Loco + HTMX settings UI. Add a macOS FileProvider for native
Finder-based file browsing. Add a redb local cache for sub-millisecond metadata
access. Add Keychain-backed secrets management on macOS.

---

## 3. Changes

### 3.1 Replace React/Tauri with Loco + HTMX Settings UI

- **Today:** React SPA (`web/src/`, 13 pages, 24 components) provides file
  browsing, search, node management, settings, and more. Requires `ts-rs` type
  generation and a REST API contract to bridge TypeScript and Rust. Tauri shell
  (`web/src-tauri/`) wraps it in a desktop webview.

- **Proposed:** Loco + HTMX server-side rendered web UI focused on **settings
  and administration only**. File browsing moves to Finder via FileProvider
  (change 3.2). The web UI handles node configuration, credential management,
  replication setup, storage backend config, and system status. Tera templates,
  no JavaScript build pipeline, no `ts-rs` type generation.

- **Justification:** With Finder handling file browsing, the web UI's scope
  shrinks dramatically — it's CRUD-heavy settings management, which is exactly
  where server-side MVC + HTMX excels. The React app, Tauri shell, and `ts-rs`
  pipeline are eliminated.

**Open item:** The exact page list for the settings UI is TBD. The current 13
React pages include file browsing, search, and dashboard views that may not be
needed once FileProvider and Finder Sync (see Deferred, §6) cover the file
interaction surface. This needs a follow-up pass to define the minimal settings
UI scope and consider how Finder Sync contextual menus might further simplify it.

### 3.2 macOS FileProvider for Native File Browsing

- **Today:** File browsing is through the React web UI or FUSE mount
  (`mosaicfs-vfs` via `fuser`). FUSE on macOS requires kernel extension
  approval and has ongoing compatibility issues with Apple Silicon and newer
  macOS versions.

- **Proposed:** A macOS FileProvider extension that presents MosaicFS files as a
  first-class Finder Location. Files appear in the Finder sidebar. Double-click
  opens directly from the local mount path. On-demand fetching for remote-only
  files (cloud icon in Finder, fetched when accessed). Change notifications via
  SSE (`GET /api/events`) triggering `NSFileProviderManager.signalEnumerator`.

- **Justification:** FileProvider is Apple's supported, sandboxed replacement for
  FUSE on macOS. It provides native Finder integration (sidebar, Quick Look,
  drag-and-drop) with no custom UI code. The developer wants a fast, smooth file
  browser — Finder already is one.

**Transport:** The FileProvider communicates with the Rust engine via its REST
API over a Unix domain socket on macOS, avoiding TCP overhead and App Transport
Security restrictions. Latency validation is a Phase 2 gate (see §4).

**No UniFFI.** Swift components are thin API clients — all logic lives in the
Rust crates, exposed via the Loco HTTP API.

### 3.3 redb Local Cache

- **Today:** Metadata reads go to CouchDB over the network.

- **Proposed:** An embedded redb key-value store providing sub-millisecond
  metadata access. Dual-write: the crawler writes to both CouchDB (federation)
  and redb (local performance). redb is the authoritative source for the
  FileProvider and Loco UI; CouchDB remains authoritative for federation.

  Data layout:
  - `inodes` table: `u64` inode ID → serialized `Inode` struct (bincode)
  - `status` table: `&str` file ID → serialized `SyncStatus` struct

- **Justification:** FileProvider has strict latency requirements for
  `enumerateItems` and `fetchContents`. CouchDB network latency is acceptable
  for a web UI but may not meet Finder's expectations. redb is pure-Rust,
  actively maintained, ACID with MVCC.

### 3.4 Single Binary with TOML Configuration

- **Today:** Two separate binaries (`mosaicfs-server`, `mosaicfs-agent`)
  deployed as separate containers. The server holds auth, TLS, and the full
  REST API surface.

- **Proposed:** A single `mosaicfs` binary. Components enabled/disabled via
  TOML configuration:

  ```toml
  [features]
  web_ui = { enabled = true, port = 8080 }
  agent = { enabled = true, watch_paths = ["/Volumes/nas"] }
  vfs = true
  ```

  Each node is self-sufficient. A NAS runs `agent + web_ui`. A Mac laptop runs
  `agent + vfs`. A headless indexer runs `agent` only.

- **Justification:** The server/agent split creates an artificial hierarchy. With
  shared crates providing all logic and CouchDB handling write coordination, no
  process needs to be privileged.

### 3.5 Peer-to-Peer Write Coordination via CouchDB MVCC

- **Today:** The server mediates most writes. Configuration changes, credential
  management, and replication setup go through the server's REST API.

- **Proposed:** All processes read and write CouchDB through shared crates.
  Conflict resolution logic lives in `mosaicfs-common` or `mosaicfs-vfs`, not
  in any particular consumer. CouchDB's revision-based MVCC detects conflicts;
  the shared crate implements deterministic resolution.

- **Justification:** CouchDB already handles concurrent writers. Moving conflict
  resolution into shared crates means every peer behaves identically — the
  server is just the peer that happens to serve HTTP.

### 3.6 Secrets Manager with Keychain Support

- **Today:** Credentials stored as plaintext in TOML config files.

- **Proposed:** A `secrets_manager` config key controlling the secrets backend:
  - `"inline"` (default) — secrets stored in the config file, as today.
  - `"keychain"` (macOS) — secret fields must be absent from config; the engine
    resolves them from Keychain using standardized item names.

  ```toml
  # inline mode (default, all platforms)
  secrets_manager = "inline"
  access_key_id = "MOSAICFS_7F3A9B2C1D4E5F6A"
  secret_key = "mosaicfs_abc123..."

  # keychain mode (macOS)
  secrets_manager = "keychain"
  access_key_id = "MOSAICFS_7F3A9B2C1D4E5F6A"
  # secret_key absent — engine reads from Keychain.
  # Startup error if secret_key is present alongside keychain mode.
  ```

  Standardized Keychain item names:

  | Item name | Maps to |
  |---|---|
  | `mosaicfs-agent-secret-key` | `agent.toml` → `secret_key` |
  | `mosaicfs-cli-secret-key` | `cli.toml` → `secret_key` |
  | `mosaicfs-backend-{backend_id}-oauth-token` | Storage backend OAuth token |

  Bootstrap flow on macOS: `POST /api/system/bootstrap` response is intercepted,
  `secret_key` stored in Keychain automatically, `agent.toml` written with only
  `access_key_id`. The user never sees the secret.

- **Justification:** Required for macOS App Sandbox and notarization. Plaintext
  secrets in config files are a sandbox violation. The `keyring` crate provides
  the implementation.

### 3.7 Extract Duplicated Code into Shared Crates

- **Today:** `couchdb.rs`, `notifications.rs`, `replication.rs`, and `readdir.rs`
  are duplicated across 2–3 crates each.

- **Proposed:** Consolidate into `mosaicfs-common` or `mosaicfs-vfs` as
  appropriate. All consumers import the shared versions.

- **Justification:** Before building more consumers of the core crates
  (FileProvider, Loco UI), clean up the foundation. This also forces
  clarification of which crate owns CouchDB interaction.

---

## 4. Implementation Phases

Per the project's "one moving part at a time" and "front-load risk" principles.

### Phase 1: Loco Bootstrap + redb Prototype

- Bootstrap Loco in `mosaicfs-agent` with a minimal status endpoint.
- Prototype redb metadata store, validate read/write latency.
- These are independent and can land together.

**Risk addressed:** Validates that Loco integrates cleanly into the existing
binary and that redb meets latency requirements.

### Phase 2: FileProvider Proof-of-Concept (macOS)

- Minimal Swift FileProvider extension that enumerates items fetched from the
  Loco REST API over a Unix domain socket.
- Validate REST latency for `enumerateItems` and `fetchContents`.
- Validate SSE-based change notifications triggering `signalEnumerator`.
- **Gate:** If REST latency over Unix socket is insufficient, adjust transport
  before proceeding.

**Risk addressed:** FileProvider latency is the highest-risk technical unknown.

**Dependencies:** Phase 1 (Loco serves the API the FileProvider consumes).

### Phase 3: Code Consolidation + Full redb Cache

- Extract duplicated modules into shared crates.
- Complete dual-write (CouchDB + redb).
- Full Loco settings UI with HTMX.

**Dependencies:** Phase 1 (Loco + redb validated).

### Phase 4: Full FileProvider Implementation

- Replace Phase 2 stub with full metadata and on-demand content fetching
  backed by redb.
- Native Finder sidebar integration.
- On-demand downloading for remote-only files.

**Dependencies:** Phase 2 (latency gate passed), Phase 3 (redb + shared crates).

### Phase 5: Unified Binary + Secrets Manager

- Merge `mosaicfs-server` and `mosaicfs-agent` into single `mosaicfs` binary.
- TOML-based feature configuration.
- Implement `secrets_manager` with Keychain backend.
- macOS: engine managed by `launchd` directly (no menu bar host).
- Update deployment manifest.

**Dependencies:** Phase 3 (shared crates clean), Phase 4 (FileProvider working).

### Phase 6: Cleanup

- Remove `web/` directory (React app, Tauri shell).
- Remove `ts-rs` dependency from workspace.
- Deprecate FUSE on macOS (Linux retains it).
- Update Dockerfile, CI.

**Dependencies:** All prior phases complete.

---

## 5. What Does Not Change

- **CouchDB** — federation and metadata store, schema, replication model.
- **S3-compatible storage** — replication backend for file content.
- **Agent crawling logic** — same behavior, lives in the unified binary.
- **FUSE/VFS on Linux** — `mosaicfs-vfs` and `fuser` remain for Linux. Only
  macOS moves to FileProvider.
- **Container deployment model** — pod structure stays (CouchDB + MosaicFS),
  fewer containers once server and agent merge.
- **TOML configuration pattern** — extended, not replaced.

---

## 6. Deferred

- **Loco settings UI page list** — the exact scope of the web UI needs a
  follow-up pass once FileProvider and Finder Sync cover the file interaction
  surface. What remains is settings/admin only, but the specific pages are TBD.
- **Finder Sync extension** (contextual menus, per-file status badges) — after
  FileProvider is proven. This could further simplify the web UI by surfacing
  common actions directly in Finder. Needs its own design pass.
- **Iced desktop app** — optional future experiment for a custom MosaicFS file
  browser with UI elements unique to MosaicFS. A "nice to have" alternative to
  Finder. Not needed until the core experience is solid.
- **Windows support** — Iced supports Windows and the unified binary would run
  there, but no Windows-specific code exists today.
- **Linux/Windows secrets backends** — `secrets_manager = "keychain"` is
  macOS-only initially. Linux (Secret Service) and Windows (Credential Manager)
  can be added later via the `keyring` crate.
- **Leptos** — considered and rejected in favor of Loco + HTMX. The settings UI
  is CRUD-heavy, which is MVC's sweet spot. Revisit only if rich client-side
  interactivity becomes a requirement.

---

## CI/CD

- **Container CI** (existing): builds the Rust workspace, runs all tests,
  produces the container image. Unaffected by macOS-native work. Active from
  Phase 1.
- **macOS CI** (added in Phase 4): validates Swift compilation and FileProvider
  extension assembly. Logic tests remain in container CI.
