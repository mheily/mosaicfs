# Change 014: Embed the agent in the desktop app

## Current State Summary

### What the desktop app looks like today (post Phase 1 of this change)

- `mosaicfs-desktop` (binary `mosaicfs-desktop`) is a Tauri 2 app that embeds
  the `mosaicfs-server` web UI in-process. After Phase 1 it also embeds
  `mosaicfs-agent` in-process — both subsystems are spawned as background
  tokio tasks from the Tauri `setup()` callback (`desktop/src/lib.rs:118-162`).
- `desktop/src/server.rs:154` builds the in-process axum router. It now
  returns `(Router, Option<String>)` so the resolved `node_id` can be passed
  to `agent::start` and both subsystems share one identity
  (`desktop/src/lib.rs:140-154`).
- `desktop/src/agent.rs:27` constructs a `MosaicfsConfig` with
  `features.agent = true` and spawns `mosaicfs_agent::start_agent` as a
  background task. It is a no-op when `settings.watch_paths` is empty, which
  is the default.
- `desktop/src/settings.rs:6` adds `watch_paths` and `excluded_paths` to
  `Settings` / `settings.json`.
- The macOS app is sandboxed with the entitlements in
  `desktop/Entitlements.plist`: `app-sandbox`, `network.client`,
  `network.server`, `files.user-selected.read-only`,
  `files.bookmarks.app-scope`. The `desktop/mosaicfs-desktop.sb` file and
  `docs/MacOS-Seatbelt.md` are reference-only documentation of the
  app's resource footprint; runtime enforcement is the kernel's App Sandbox
  driven by those entitlements.

### Surrounding pieces this change builds on

- `mosaicfs_agent::start_agent` (`mosaicfs-agent/src/start.rs:30`) runs an
  initial crawl, then a periodic heartbeat / crawl / health-check `select!`
  loop. It has no externally-driven cancellation — its only exit path is the
  internal `shutdown_signal()` waiting on `SIGTERM` / `Ctrl-C`
  (`start.rs:138-184`).
- `mosaicfs_agent::crawler::crawl` (`crawler.rs:28`) takes
  `&[PathBuf]` for both watch and exclude paths and walks them with
  `WalkDir`. It does not coordinate with any per-path access lifecycle.
- `desktop/src/bookmarks.rs:15` — `BookmarkStore` keyed by canonical path
  string, value is the `bookmarkDataWithOptions` blob, persisted as
  base64 JSON to `bookmarks.json` in the app data dir. Already used for VFS
  mount authorization.
- `desktop/src/macos.rs:14` — `ResolvedUrl` RAII guard that holds an
  active `startAccessingSecurityScopedResource` and releases it on drop.
- `desktop/src/commands.rs:209` — existing `authorize_mount` Tauri command
  that pre-canonicalizes a path, runs `NSOpenPanel` on the main thread,
  verifies the user's selection matches, calls `create_bookmark`, and
  inserts into `BookmarkStore`. The flow needed for watch paths is
  structurally identical.
- `desktop/ui/setup.html` — the existing pattern for desktop-only dialogs:
  a static HTML+JS page bundled with the app and opened as
  `tauri::WebviewUrl::App(...)` in a Tauri window. The server-rendered
  Tera+HTMX UI is the convention for UI hosted by `mosaicfs-server`; this
  static-HTML pattern is the established exception for windows that
  need to call Tauri commands and that exist only in the desktop app.

### Code paths that confirm the discussion's blockers

- The sandbox-blocks-watch-paths claim: `Entitlements.plist` only declares
  `files.user-selected.read-only` and `files.bookmarks.app-scope`. There is
  no `temporary-exception` for absolute paths and no `files.all` style
  entitlement. Anything not opened via `NSOpenPanel` and held by a resolved
  bookmark is denied at the kernel level.
- The no-cancellation claim: `start_agent` consumes `Arc<MosaicfsConfig>`
  and a `SecretsBackend`. No `CancellationToken`, channel, or stop flag.
  The `save_settings` Tauri command rebuilds the router and explicitly does
  not touch the agent (`desktop/src/lib.rs:97-101`).

## Goal

Make the in-process agent in the desktop app actually crawl files. Phase 1
of change 014 wired the agent up structurally — the process starts, the loop
runs, the configured `watch_paths` are read — but on macOS no crawl walks
anywhere because every path the user names is outside the sandbox container
and silently denied. This change adds the access lifecycle (security-scoped
bookmarks per watch path) and the UI to create those bookmarks.

## Changes

### 1. Crawler accepts opened watch paths instead of bare config paths

- **Today:** `mosaicfs_agent::crawler::crawl` takes `&[PathBuf]` and runs
  `WalkDir::new(...)` on each one (`crawler.rs:49-66`). The agent's main loop
  calls it with `&agent_cfg.watch_paths` directly (`start.rs:77-84`,
  `start.rs:124-130`). The crawler has no notion of "this path needs to be
  opened first and closed afterwards" — it assumes the process can read the
  path at any time.
- **Proposed:** Introduce a small "watch path provider" abstraction that the
  agent loop calls before each crawl tick. The provider returns a vector of
  values that pair the `PathBuf` to crawl with an opaque RAII guard that
  keeps the access alive for the duration of the call. The crawler still
  takes paths but the loop holds the guards across the call.

  Sketch (in `mosaicfs-agent`, no platform crate dependencies):
  ```rust
  pub trait WatchPathProvider: Send + Sync {
      fn open(&self) -> anyhow::Result<Vec<OpenedWatchPath>>;
  }
  pub struct OpenedWatchPath {
      pub path: PathBuf,
      pub _guard: Box<dyn Any + Send + Sync>,
  }
  ```
  - Linux / containerized server: trivial provider that returns
    `agent_cfg.watch_paths.clone()` paired with `Box::new(())` guards. No
    behaviour change.
  - Desktop on macOS: provider that, for each entry in `watch_paths`, looks
    up the bookmark in `BookmarkStore`, calls `resolve_bookmark` to get a
    `ResolvedUrl`, and returns the resolved path with the `ResolvedUrl`
    boxed as the guard. Paths with no bookmark are skipped and logged
    (`tracing::warn!`).
- **Justification:** Without this, the desktop crawler is structurally
  unable to access anything in `watch_paths` on macOS — every read returns
  ENOENT/EPERM at the kernel level because the process never called
  `startAccessingSecurityScopedResource` for the URL. Putting the open/close
  inside the crawler crate would force `mosaicfs-agent` to depend on
  `objc2` / Cocoa, which would break the Linux build and entangle a shared
  crate with platform code. A small provider trait keeps the platform code
  in `desktop/`.

### 2. `start_agent` accepts a `WatchPathProvider`

- **Today:** `start_agent(cfg, secrets)` is the only entry point. Internally
  it reads `agent_cfg.watch_paths` and passes it to `crawler::crawl` as a
  raw slice. The desktop's `agent::start` calls `start_agent` directly
  (`desktop/src/agent.rs:62`).
- **Proposed:** Change the signature to
  `start_agent(cfg, secrets, provider: Arc<dyn WatchPathProvider>)`. The
  agent loop calls `provider.open()` once for the initial crawl and once
  per crawl tick, keeps the returned `OpenedWatchPath` vector alive across
  the call, and discards it (releasing access) when the call returns. The
  Linux path defaults to `Arc::new(BareWatchPathProvider::from(cfg.clone()))`
  — `start_web_ui` / containerized server change at one call site.
- **Justification:** The agent loop is the only thing positioned to scope
  access across the crawl. Scoping access inside the crawler (per file)
  would mean re-resolving and re-opening on every directory descent, which
  is unnecessarily expensive and wrong: the bookmark resolves to the watch
  path root, not the descendants.

### 3. Desktop `WatchPathProvider` resolves bookmarks

- **Today:** No watch-path bookmarks are created or consulted anywhere. The
  desktop calls `start_agent` with the bare `watch_paths` from settings.
- **Proposed:** A new `desktop/src/watch_paths.rs` (or method on the
  existing `BookmarkStore`) implements `WatchPathProvider`. Per call:
  for each path in `settings.watch_paths`, look up the bookmark by
  canonical path, call `resolve_bookmark` (which is already an RAII guard
  via `ResolvedUrl`), and return paired entries. Stale-bookmark resolution
  errors are logged and the path is skipped (it will reappear on the next
  tick if the user re-authorizes via the UI). The provider holds an
  `Arc<Mutex<BookmarkStore>>` clone — the same store the existing
  `authorize_mount` uses — and no new persistence layer is introduced.
- **Justification:** This is the desktop-side half of change 1. Without
  it, the new trait API is no use to the only platform that needed it.

### 4. Agent settings Tauri window

- **Today:** `desktop/ui/setup.html` is the only desktop-only window; it
  configures CouchDB and is opened from the tray menu's "Connection…" item
  (`desktop/src/lib.rs:32-47`, `lib.rs:209-218`). There is no UI for
  `watch_paths`. The discussion file explicitly states the JSON-edit
  workaround it documented does not actually work, because bookmarks
  cannot be created without `NSOpenPanel`.
- **Proposed:** A new `desktop/ui/agent.html` window opened from a new tray
  menu item (working name: "Watch Folders…"). The window lists each entry
  in `settings.watch_paths`, shows whether a bookmark is present
  ("Authorized" / "Needs authorization"), and exposes:
  - **Add folder…** — opens `NSOpenPanel`, captures the user's selection
    canonically, appends to `watch_paths`, creates and stores the
    bookmark in one step.
  - **Authorize…** (per existing path with no bookmark) — opens
    `NSOpenPanel` pre-selected at the path, validates the selection
    matches, creates and stores the bookmark.
  - **Remove** (per path) — removes the path from `watch_paths` and
    removes the corresponding bookmark from the store.

  The window calls a new set of Tauri commands that mirror the shape of
  `authorize_mount`:
  - `list_watch_paths()` → `Vec<{ path: String, authorized: bool }>` —
    composes settings + bookmark store.
  - `add_watch_path()` → opens panel, validates, stores both. The
    selection becomes the canonical path; if the user cancels, the
    settings file is untouched.
  - `authorize_watch_path(path: String)` → opens panel pre-selected at
    `path`, validates match, stores bookmark.
  - `remove_watch_path(path: String)` → removes from settings and from
    bookmark store atomically (settings write first, then bookmark; if
    the bookmark removal fails, it's logged and reconciled on next
    `list_watch_paths()`).

  The window shows a banner: "Restart MosaicFS to apply changes." (see
  Deferred for the dynamic-restart story).
- **Justification:** Bookmarks must be created via `NSOpenPanel`, which
  must be driven from the main thread of the desktop process. There is no
  way to bootstrap them by editing a JSON file. The Tauri window is the
  established pattern for desktop-only dialogs (see `setup.html`); reusing
  that pattern keeps the change scoped to the desktop crate. Putting this
  page in the server-rendered Tera UI would require new server↔Tauri
  plumbing for an interaction that is fundamentally desktop-local
  (settings.json + Cocoa main-thread modal), which is the kind of layering
  the project decisions warn against.

### What does not change in `mosaicfs-server` or the Linux deployment

- The server router and its `/api/agent/*` endpoints are unchanged. The
  agent settings UI is desktop-local and does not go through the REST API.
- The Linux container deployment in `deploy/mosaicfs.yaml` is unaffected;
  the new `WatchPathProvider` trait carries a default bare implementation
  used by `mosaicfs-server`'s agent startup path.
- `mosaicfs-agent` does not gain any platform-conditional code or platform
  dependencies. The macOS-specific bookmark logic stays in `desktop/`.

## Implementation Phases

Phases here are organized by topical clarity. Per project convention, the
tree may not be functionally complete between phases — only the final state
needs to work end to end.

### Phase 1 — Bookmark-aware crawler driver (in `mosaicfs-agent`)

Deliverables:
- New `WatchPathProvider` trait and `OpenedWatchPath` struct in
  `mosaicfs-agent` (top-level `lib.rs` re-exports).
- `BareWatchPathProvider { paths: Vec<PathBuf> }` implementation used by
  the existing server agent startup.
- `start_agent` signature changed to accept `Arc<dyn WatchPathProvider>`.
  Loop calls `provider.open()` for the initial crawl and once per crawl
  tick. Returned `OpenedWatchPath` vector is held across the `crawl(...)`
  call and dropped before the next `tokio::select!` iteration.
- The single existing call site that drives the server agent (in
  `mosaicfs-server::start::start_web_ui` or wherever the server spawns the
  agent in the unified-binary path) constructs a `BareWatchPathProvider`
  from `cfg.agent.watch_paths.clone()` and passes it through.
- `desktop/src/agent.rs` is updated to pass a placeholder
  `BareWatchPathProvider` so the desktop still compiles. This phase does
  not yet improve macOS behaviour — the provider hookup happens in Phase 2.
- Crawler unit tests, if any are added, exercise the bare provider against
  a `tempfile::tempdir`.

Cross-phase dependency: Phase 2 needs Phase 1's trait and signature change
to be in place.

### Phase 2 — Desktop bookmark resolver and watch-path UI

Deliverables:
- `desktop/src/watch_paths.rs` (new): `BookmarkedWatchPathProvider` that
  holds an `Arc<Mutex<BookmarkStore>>` and a snapshot of
  `settings.watch_paths` (refreshed each `open()` call against
  `settings::load`). For each path, look up the bookmark, call
  `macos::resolve_bookmark`, and box the resulting `ResolvedUrl` as the
  guard. On stale or missing, log and skip; do not propagate error so a
  single bad path doesn't abort the whole tick.
- `desktop/src/agent.rs` updated to construct
  `BookmarkedWatchPathProvider` instead of the bare one and pass it to
  `start_agent`. Unchanged: agent skipped when `watch_paths` is empty.
- `desktop/ui/agent.html` (new): static HTML+JS modeled on
  `setup.html`. Lists watch paths with status badges and the three
  actions described in change 4. Layout follows the existing window's
  visual conventions; no new framework or asset pipeline.
- `desktop/src/lib.rs`: new tray menu item ("Watch Folders…") opening the
  new window the same way "Connection…" opens setup; new Tauri commands
  registered in the `invoke_handler` array; window definition added next
  to `open_setup_window`.
- New Tauri commands: `list_watch_paths`, `add_watch_path`,
  `authorize_watch_path`, `remove_watch_path`. Implementations live in
  `desktop/src/commands.rs` next to `authorize_mount` and re-use the
  existing `MacosApi` trait so they remain unit-testable on non-macOS.
- Settings writes go through `settings::save` (atomic temp+rename
  pattern matches `BookmarkStore::save`). The UI does not invent its own
  serialization.
- After any add/authorize/remove, the UI shows the "Restart MosaicFS to
  apply changes" banner. The user closes the window and quits/relaunches
  the app via the tray (or the menu). No process management is added.

End state after Phase 2: a fresh install can configure CouchDB, add a
folder via "Watch Folders…", authorize it, restart the app, and see file
documents appear in CouchDB for the authorized folder.

## What Does Not Change

- `mosaicfs-server` REST API surface, Tera templates, and `/ui/*` routes.
- `mosaicfs-agent`'s replication subsystem, heartbeat, node-registration,
  and CouchDB write paths. The crawler walks the same way; only the
  caller decides what paths to walk and keeps access open during the call.
- `mosaicfs-common`: `MosaicfsConfig`, `AgentFeatureConfig`,
  `CouchdbConfig`, `Settings` (server-side `[agent].watch_paths` typing
  is unchanged).
- `BookmarkStore` schema, file format, and version (still version 1, still
  base64 in JSON, still keyed by canonical path string). Watch path
  bookmarks share the same key namespace as VFS mount bookmarks; no
  collision is possible in practice because both keys are canonical
  filesystem paths owned by the same user.
- `desktop/Entitlements.plist`. The existing
  `files.user-selected.read-only` + `files.bookmarks.app-scope` pair is
  exactly what this design needs. No new entitlement is requested.
- The CouchDB connection setup window (`setup.html`) and its commands
  (`save_settings`, `test_connection`).
- The container deployment (`deploy/mosaicfs.yaml`) and the Linux build
  story. Phase 1's `BareWatchPathProvider` keeps the server's agent start
  path behaviour-identical.
- `Dockerfile.mosaicfs` and the CI build. No new Rust crates, no JS
  toolchain, no new static assets beyond the bundled HTML file in
  `desktop/ui/`. The desktop build's only added Tauri command bindings
  are exposed automatically by `tauri::generate_handler!`.

## Deferred

- **Hot-reload of `watch_paths` without restart.** Refactor
  `start_agent` to accept a `tokio_util::sync::CancellationToken`, store
  the agent's `JoinHandle` in Tauri-managed state, and have the new
  Tauri commands cancel-and-respawn after each settings write. Adds a
  workspace dependency (`tokio-util`) and a non-trivial supervisor in
  the desktop crate. Not needed for the agent to *work*; the user can
  quit-and-relaunch via the tray. Cost is one quit-relaunch per
  watch-path edit, which is fine for a local-first config flow.
- **Linux desktop / GTK build of the watch-folders UI.** No sandbox
  forces this on Linux, and the Linux build path today goes through the
  containerized agent. Adding a desktop-on-Linux variant is a separate
  product decision.
- **Surface crawl progress / "skipped: no bookmark" status in the UI.**
  Today the agent only logs to the tracing subscriber. A status line per
  path ("Last crawl: 12:03, 4,182 files indexed") would help the user
  but is independent from the access-lifecycle problem this change
  exists to solve.
- **Dedicated agent settings page in the server-rendered Tera UI.** If
  a future direction unifies all settings into the server UI, the
  desktop's static-HTML window is replaced rather than ported. Premature
  to design until that direction is settled.
- **Per-path TCC / Full Disk Access prompts.** A more permissive sandbox
  posture (e.g., requesting Full Disk Access at install time) would let
  the user skip per-folder authorization entirely. The discussion file
  is explicit that the sandbox stays; revisit only if the per-folder
  flow proves unworkable.
