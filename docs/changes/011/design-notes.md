# Change 011: Design Notes

Implementation plan for `architecture-v2.md`. Consumed by the implementing
agent (Sonnet 4.6 / Qwen 3.6). Organized phase-by-phase, with per-step file
paths, function signatures, and unit tests.

**Before starting, read:** `architecture-v2.md` in the same directory.
This document assumes familiarity with the six numbered changes (1, 1a, 2,
3, 4, 5).

---

## Shared Contracts

These types/shapes are referenced by multiple phases. Implement them
once and reuse.

### JSON wire format — `POST /ui/browse/open`

**Request body** (unchanged from today):

```json
{ "path": "/virtual/path/to/file.txt" }
```

**Success response** (new shape, `200 OK`):

```json
{
  "node_id": "<source-node-uuid>",
  "local_mount_path": "/Volumes/NAS",
  "relative_path": "photos/2024/IMG_0001.jpg"
}
```

**Invariants the server guarantees:**
- `relative_path` never begins with `/`. `PathBuf::join` treats a
  leading-`/` component as absolute and silently drops the preceding
  mountpoint — so if the server ever emitted `/photos/…` the Tauri
  traversal check would be bypassed. The resolver strips the leading
  slash before returning. The Tauri side does **not** re-strip; it just
  calls `canonical_mount.join(&relative_path)` and trusts the contract.
- `local_mount_path` is already canonical (Change 1a normalizes on write).

**Error response** (new shape; one HTTP status per error variant):

```json
{ "code": "no_host_mount", "message": "No local mount for node X …", "node_id": "X" }
```

| Variant                | HTTP | `code`              | Extra fields         |
|------------------------|------|---------------------|----------------------|
| `NotFound(file_id)`    | 404  | `not_found`         | `file_id`            |
| `NoNodeId`             | 500  | `no_node_id`        | —                    |
| `NodeNotRegistered`    | 500  | `node_not_registered`| —                   |
| `NoHostMount(node_id)` | 409  | `no_host_mount`     | `node_id`            |

All error bodies carry a human-readable `message` string.

### Tauri command contracts

```rust
// desktop/src/commands.rs
#[derive(serde::Deserialize)]
pub struct OpenTarget {
    pub node_id: String,
    pub local_mount_path: String,
    pub relative_path: String,  // never begins with '/' — see invariant above
}

#[derive(serde::Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum OpenError {
    BookmarkNotAuthorized { local_mount_path: String, node_id: String },
    PathNotAccessible { local_mount_path: String, relative_path: String },
    PathTraversal {
        requested_path: String,
        resolved_path: String,
        local_mount_path: String,
    },
    OpenFailed { message: String },
}

#[derive(serde::Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AuthorizeError {
    UserCancelled,
    MismatchedSelection { expected: String, got: String },
    BookmarkCreationFailed { message: String },
}

#[tauri::command]
pub async fn open_file(target: OpenTarget) -> Result<(), OpenError> { … }

#[tauri::command]
pub async fn authorize_mount(local_mount_path: String) -> Result<(), AuthorizeError> { … }
```

`#[serde(tag = "code")]` makes each variant serialize as
`{ "code": "bookmark_not_authorized", "local_mount_path": "…", "node_id": "…" }`
— parallel to the server's error shape, so the JS handler can branch on
`code` uniformly regardless of origin.

### Bookmark store on disk

File: `<AppData>/mosaicfs-desktop/bookmarks.json` (resolve via
`tauri::Manager::path().app_data_dir()`).

```json
{
  "version": 1,
  "bookmarks": {
    "/Volumes/NAS": "<base64 bookmark data>",
    "/Volumes/Archive": "<base64 bookmark data>"
  }
}
```

On load:
- File missing → empty store (`{ version: 1, bookmarks: {} }` in memory).
- `version != 1` → empty store + `tracing::warn!`; do not delete the file
  (let a future version migrate).
- Parse error on a file that does exist → empty store + `tracing::error!`;
  preserve the file (rename to `bookmarks.json.corrupt.<timestamp>` so the
  user can recover manually).

On save: atomic rename (`tempfile` then `rename`) to avoid torn writes.

---

## Phase 1 — Server-side: descriptor-returning resolver

### Files touched

- `mosaicfs-server/src/ui/open.rs` — rewrite
- `mosaicfs-server/src/ui/browse.rs` — change `open` handler response
- `mosaicfs-server/src/ui/actions.rs` — remove dead import
- `mosaicfs-server/src/handlers/nodes.rs` — add normalization in
  `add_mount` + `patch_mount`
- `mosaicfs-agent/src/...` — add normalization wherever `storage[]
  .mount_point` is written (find via `grep -rn storage.*mount_point`)
- New test module: `mosaicfs-server/src/ui/open.rs` (`#[cfg(test)] mod tests`)

### Steps

1. **Rewrite `ui/open.rs`.**
   - Replace the existing `OpenError` with four variants:
     `NotFound(file_id: String)`, `NoNodeId`, `NodeNotRegistered`,
     `NoHostMount { source_node_id: String }`.
   - Define `pub struct OpenTarget { pub node_id: String,
     pub local_mount_path: String, pub relative_path: String }` with
     `#[derive(Serialize, Debug, Clone, PartialEq)]`.
   - Rewrite `open_file_by_id` to return `Result<OpenTarget, OpenError>`:
     - Fetch the file document; `NotFound` on error.
     - Extract `source.node_id` and `source.export_path`.
     - If `state.node_id.as_deref() == Some(&source_node_id)`: same-node
       resolution. Fetch `node::<source_node_id>`, walk `storage[]`, find
       the entry whose `mount_point` is the longest prefix of
       `source.export_path`. Return `{ node_id, local_mount_path:
       mount_point, relative_path: (export_path - mount_point)
       .trim_start_matches('/') }`.
       If no entry matches: `NoHostMount { source_node_id }`. **Note:**
       this is a behavior change from the current `open.rs:56`, which
       returns `export_path` verbatim for same-node files. See the
       "Behavior change from current code" note in architecture-v2.md
       Change 1.
     - Else: cross-node resolution via `network_mounts` (preserve the
       existing longest-`remote_base_export_path` + highest-`priority`
       matching from lines 77–94 of the current file). Return
       `{ node_id: source_node_id, local_mount_path:
       network_mount.local_mount_path, relative_path: (source.export_path
       - remote_base_export_path).trim_start_matches('/') }`.
     - **Before returning:** assert (debug-only) that `relative_path` does
       not begin with `/`. This is a wire-contract invariant; a violation
       indicates a resolver bug.
   - **Delete** the existence check, the `Command::new` spawn, the
     `SpawnFailed` / `PathNotAccessible` / `NoMount` variants, and the
     `summarize_open_error` helper. They move to the Tauri app (Phase 2).

2. **Update `ui/browse.rs::open`** (lines 211–230):
   - Change the return type to `Response` returning JSON.
   - On `Ok(target)`: return `(StatusCode::OK, Json(target)).into_response()`.
   - On `Err(e)`: map to an `(status, Json(body))` tuple per the table in
     Shared Contracts. Use a helper `fn error_response(e: &OpenError) ->
     Response` colocated in `browse.rs` or `open.rs`.
   - Drop the `flash_response` usage for this handler.
   - Keep `lookup_entry_by_virtual_path` as-is. If the virtual path
     doesn't resolve, return `404 { "code": "not_found", "message":
     "virtual path does not exist" }`.

3. **`ui/actions.rs:22`** — remove `open_file_by_id` from the `use`
   statement. Verify the file still compiles (`open_file_by_id` must not
   be referenced anywhere in `actions.rs`).

4. **`handlers/nodes.rs::add_mount` + `patch_mount`** (Change 1a):
   - Add `fn normalize_mount_path(input: &str) -> String`:
     - Trim trailing `/` except when the input is exactly `"/"`.
     - Collapse runs of `/` into one (`foo//bar` → `foo/bar`).
     - Attempt `std::fs::canonicalize`; on success use that string, on
       error keep the normalized-but-unresolved form.
   - Call the normalizer before writing `local_mount_path` in both
     handlers.

5. **Agent side (if applicable).** `grep -rn 'mount_point' mosaicfs-agent/
   src/` to find where `storage[]` is populated. Apply the same
   `normalize_mount_path` there. If the normalizer ends up used by both
   crates, place it in `mosaicfs-common` (new file `paths.rs`) and
   re-export. Do **not** duplicate.

### Unit tests (Phase 1)

Place in `mosaicfs-server/src/ui/open.rs` (test module) unless noted.
Each test should construct the minimum CouchDB state it needs via test
fixtures. If the existing test helpers in the crate use a mock db, reuse
them; if not, add one using `serde_json::json!` to fabricate documents.

| # | Test                                           | Assertion                                                                                                                |
|---|------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------|
| T1.1 | `open_file_by_id` on unknown file_id        | Returns `Err(OpenError::NotFound(id))` where `id` matches input                                                          |
| T1.2 | Cross-node: exact match                        | Given file's `source.export_path = "/export/photos/a.jpg"` and a `network_mount { remote_base: "/export", local: "/Volumes/NAS" }`, returns `OpenTarget { local_mount_path: "/Volumes/NAS", relative_path: "/photos/a.jpg", … }` |
| T1.3 | Cross-node: longest-prefix match              | With two mounts `/export` → `/Volumes/A` and `/export/photos` → `/Volumes/B`, for file in `/export/photos/x`, picks `/Volumes/B` |
| T1.4 | Cross-node: priority tiebreak                  | Two mounts with equal `remote_base` length → highest `priority` wins                                                     |
| T1.5 | Cross-node: no matching mount                  | Returns `Err(OpenError::NoHostMount { source_node_id })`                                                                 |
| T1.6 | Cross-node: server has no `node_id` configured | Returns `Err(OpenError::NoNodeId)`                                                                                       |
| T1.7 | Same-node: single storage entry                | `state.node_id == source.node_id`, one storage entry `mount_point: "/data"`, file at `/data/x.txt` → `local_mount_path: "/data", relative_path: "/x.txt"` |
| T1.8 | Same-node: longest storage prefix              | Two storage entries `/data` and `/data/archive`, file at `/data/archive/old.txt` → picks `/data/archive`                |
| T1.9 | Same-node: no matching storage entry           | Returns `Err(OpenError::NoHostMount { source_node_id })`                                                                 |
| T1.10 | `normalize_mount_path` cases                 | `"/foo//bar/"` → `"/foo/bar"`; `"/"` → `"/"`; `""` → `""`; non-existent path passes through normalization unchanged      |
| T1.10a | `relative_path` invariant                   | For every success return across T1.2, T1.3, T1.4, T1.7, T1.8: assert `!target.relative_path.starts_with('/')` |

**Handler-level tests** (place wherever browse handler tests live today —
check for an existing `#[cfg(test)] mod tests` in `browse.rs`; otherwise
add one):

| # | Test                                                | Assertion                                                                              |
|---|-----------------------------------------------------|----------------------------------------------------------------------------------------|
| T1.11 | Handler success                                   | `POST /ui/browse/open` with valid path → 200, body parses as `OpenTarget`              |
| T1.12 | Handler error surface: unknown virtual path       | 404, body `{ "code": "not_found", … }`                                                 |
| T1.13 | Handler error surface: no host mount              | 409, body `{ "code": "no_host_mount", "node_id": "…", … }`                             |
| T1.14 | Handler sets `Content-Type: application/json`     | on both success and error                                                              |

**Acceptance check (manual, between phases):**
```sh
curl -X POST -F 'path=/some/known/path' http://localhost:8443/ui/browse/open
# → JSON OpenTarget or JSON error body with correct status.
grep -rn 'Command::new("open"' mosaicfs-server/src/ui/
grep -rn 'Command::new("xdg-open"' mosaicfs-server/src/ui/
# → zero hits.
```

---

## Phase 2 — Tauri-side: commands + bookmark store

### Files touched

- `desktop/Cargo.toml` — add dependencies
- `desktop/Entitlements.plist` — replace contents
- `desktop/capabilities/default.json` — whitelist commands
- `desktop/src/lib.rs` — register handlers
- `desktop/src/commands.rs` — new
- `desktop/src/bookmarks.rs` — new
- `desktop/src/macos.rs` — new (macOS-only `cfg_attr(target_os = "macos")`)
- `desktop/src/stub.rs` — new (non-macOS stub)

### Dependency additions (`desktop/Cargo.toml`)

```toml
[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"
thiserror = "1"
tracing = "0.1"

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-foundation = { version = "0.2", features = ["NSURL", "NSData", "NSString", "NSError", "NSArray"] }
objc2-app-kit = { version = "0.2", features = ["NSOpenPanel", "NSWorkspace", "NSApplication"] }
block2 = "0.5"
```

(Agent: confirm current objc2 version on crates.io before committing;
pin to latest 0.5.x / 0.2.x as appropriate. Do not use `cocoa` or the
legacy `objc` crate — `objc2` is the maintained successor.)

### Entitlements (`desktop/Entitlements.plist`, full replacement)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.app-sandbox</key><true/>
    <key>com.apple.security.network.client</key><true/>
    <key>com.apple.security.files.user-selected.read-only</key><true/>
    <key>com.apple.security.files.bookmarks.app-scope</key><true/>
</dict>
</plist>
```

### Capabilities (`desktop/capabilities/default.json`)

Add to the `permissions` array:
```json
"core:default",
"core:event:default"
```
and add a new section:
```json
"commands": ["open_file", "authorize_mount"]
```
(Agent: verify exact Tauri 2 capabilities schema — may be expressed
differently; the goal is the two commands callable from the `main`
window.)

### Steps

1. **`bookmarks.rs`** — implement the versioned store:
   ```rust
   pub struct BookmarkStore { path: PathBuf, map: HashMap<String, Vec<u8>> }

   impl BookmarkStore {
       pub fn load(path: PathBuf) -> Self;               // graceful fallback per contract
       pub fn get(&self, mount: &str) -> Option<&[u8]>;
       pub fn insert(&mut self, mount: String, data: Vec<u8>) -> io::Result<()>; // persists
       pub fn remove(&mut self, mount: &str) -> io::Result<()>;                  // persists
       fn save(&self) -> io::Result<()>;                  // atomic rename
   }
   ```
   The map key is the canonicalized `local_mount_path` string. Base64
   encoding is applied at serialize time; in-memory holds raw bytes.

2. **`macos.rs`** — implement:
   ```rust
   // Called synchronously ON the main thread. Must not be invoked off-main.
   pub fn show_open_panel_sync(preselect: &Path) -> Option<PathBuf>;

   pub fn create_bookmark(url: &Path) -> Result<Vec<u8>, String>;
   pub fn resolve_bookmark(data: &[u8]) -> Result<ResolvedUrl, ResolveBookmarkError>;
   pub struct ResolvedUrl { /* holds NSURL; dropping stops access */ }
   pub fn nsworkspace_open(path: &Path) -> bool;

   pub enum ResolveBookmarkError { Stale, Other(String) }
   ```
   - `ResolvedUrl::new` calls `startAccessingSecurityScopedResource`; its
     `Drop` calls `stopAccessingSecurityScopedResource`. Never expose the
     NSURL outside this module — only `as_path(&self) -> &Path`.
   - `create_bookmark` sets options `NSURLBookmarkCreationWithSecurityScope`;
     resolves with `NSURLBookmarkResolutionWithSecurityScope` +
     `NSURLBookmarkResolutionWithoutUI`.
   - `nsworkspace_open` calls `[NSWorkspace sharedWorkspace] openURL:` with
     a `fileURLWithPath:` NSURL; returns the BOOL.
   - **`show_open_panel_sync` must be called on the main thread.**
     `NSOpenPanel.runModal()` asserts main-thread. Tauri 2 async commands
     run on a tokio thread pool, so the command wrapper in `commands.rs`
     must bridge via `AppHandle::run_on_main_thread` + a oneshot channel:
     ```rust
     // inside authorize_mount (async)
     let (tx, rx) = tokio::sync::oneshot::channel();
     let preselect_clone = preselect.clone();
     app.run_on_main_thread(move || {
         let _ = tx.send(macos::show_open_panel_sync(&preselect_clone));
     })
     .map_err(|e| AuthorizeError::BookmarkCreationFailed {
         message: format!("run_on_main_thread: {e}"),
     })?;
     let selection: Option<PathBuf> = rx.await
         .map_err(|e| AuthorizeError::BookmarkCreationFailed {
             message: format!("oneshot recv: {e}"),
         })?;
     ```
     The blocking `runModal` call holds the main thread until the user
     dismisses the panel; that's fine because we're on the main queue, not
     the tokio runtime.
   - `nsworkspace_open` is documented as thread-safe in AppKit, but
     `NSWorkspace` state mutations are not — keep the call on the
     command's tokio thread for now (no need to dispatch). If this turns
     out to be flaky in practice, wrap it in the same
     `run_on_main_thread` pattern.

3. **`stub.rs`** (non-macOS):
   Every function returns `OpenError::OpenFailed { message: "desktop open
   not implemented on this platform".into() }` or the equivalent
   `AuthorizeError`.

4. **`commands.rs`** — implement `open_file`:
   - Canonicalize `target.local_mount_path`. If canonicalization fails
     (path doesn't exist on disk), treat as `BookmarkNotAuthorized` so the
     user can re-pick.
   - Look up bookmark by canonical key. Missing → `BookmarkNotAuthorized`.
   - `resolve_bookmark`. On `ResolveBookmarkError::Stale`: remove from
     store, return `BookmarkNotAuthorized`. On `Other`: return
     `OpenFailed`.
   - Compute `requested = canonical_mount.join(&target.relative_path)`.
     The server's contract guarantees `relative_path` has no leading `/`,
     so `join` appends under the mountpoint. Do **not** strip or
     re-normalize here — trust the boundary.
   - `resolved = std::fs::canonicalize(&requested)`. On NotFound →
     `PathNotAccessible`. On other I/O error → `OpenFailed`.
   - Verify `resolved.starts_with(&canonical_mount)`. If not →
     `PathTraversal { requested_path: requested.display().to_string(),
     resolved_path: resolved.display().to_string(), local_mount_path:
     canonical_mount.display().to_string() }`.
   - Verify `resolved.exists()` (canonicalize succeeded, so this should
     always be true — but belt-and-suspenders). If not →
     `PathNotAccessible`.
   - `nsworkspace_open(&resolved)`. On `false` → `OpenFailed`.
   - Return `Ok(())`.

5. **`commands.rs`** — implement `authorize_mount`:
   - Canonicalize requested path; use the as-given path as
     `NSOpenPanel.directoryURL` if canonicalize fails (e.g. mount
     currently offline — user may still be able to navigate there).
   - Call `show_open_panel_sync` via the oneshot bridge (Step 2). `None`
     → `UserCancelled`.
   - Canonicalize the user's selection. If it differs from the
     canonicalized requested path: return `MismatchedSelection { expected:
     canonical_requested.display().to_string(), got:
     canonical_selection.display().to_string() }`. **Return once — no
     loop.** The UI surfaces the mismatch as a flash with a "Try again"
     button, and the user clicks to re-invoke `authorize_mount`. This
     keeps the command single-shot and the UI in control of retries.
   - `create_bookmark` on the selection. On error →
     `BookmarkCreationFailed`.
   - `store.insert(canonical_mount.display().to_string(), data)`.
   - `Ok(())`.

6. **`lib.rs`** — grow the builder:
   ```rust
   pub fn run() {
       tauri::Builder::default()
           .setup(|app| {
               let store_path = app.path().app_data_dir()?.join("bookmarks.json");
               std::fs::create_dir_all(store_path.parent().unwrap()).ok();
               let store = BookmarkStore::load(store_path);
               app.manage(Mutex::new(store));
               WebviewWindowBuilder::new(…).build()?;
               Ok(())
           })
           .invoke_handler(tauri::generate_handler![
               commands::open_file,
               commands::authorize_mount,
           ])
           .run(tauri::generate_context!())
           .expect("error while running tauri application");
   }
   ```

### Unit tests (Phase 2)

Tauri commands that call into objc2 can't be pure-Rust unit tested on
CI (would need a windowing session). Split the logic accordingly:

**Pure-logic tests** (`bookmarks.rs` + a `logic.rs` module holding
command bodies parameterized over a `MacosApi` trait):

| # | Test                                     | Assertion                                                                                              |
|---|------------------------------------------|--------------------------------------------------------------------------------------------------------|
| T2.1 | `BookmarkStore::load` missing file      | Returns empty store, no panic                                                                          |
| T2.2 | `BookmarkStore::load` version mismatch  | Given `{"version": 99, "bookmarks": {"/x":"yyy"}}` → empty store, file untouched on disk                |
| T2.3 | `BookmarkStore::load` corrupt JSON      | Given invalid JSON → empty store, original file renamed to `.corrupt.<ts>`                              |
| T2.4 | `BookmarkStore::insert` + reload        | After insert + drop + reload, the bookmark round-trips (base64 encode/decode is correct)                |
| T2.5 | `BookmarkStore::save` atomicity         | Simulate partial write (fake temp file with injected failure) → original file intact                    |
| T2.6 | open_file logic: bookmark missing       | `MacosApi` stub reports no bookmark → returns `BookmarkNotAuthorized`                                   |
| T2.7 | open_file logic: bookmark stale         | `MacosApi::resolve_bookmark` returns `Stale` → returns `BookmarkNotAuthorized`, store entry removed     |
| T2.8 | open_file logic: path traversal via fake symlink | With a tmpdir: `mount/a` is a symlink to `../outside`, `relative_path="a/x"` → `PathTraversal` with both paths populated |
| T2.9 | open_file logic: file not found         | `relative_path` points to nonexistent file → `PathNotAccessible`                                        |
| T2.10 | open_file logic: happy path            | Stub `nsworkspace_open` returns `true`, everything else OK → `Ok(())`, and `nsworkspace_open` is called once with the canonicalized path |
| T2.11 | authorize_mount: user cancels          | Stub panel returns `None` → `UserCancelled`                                                            |
| T2.12 | authorize_mount: mismatched selection  | Stub panel returns a different directory than requested → `MismatchedSelection { expected, got }`, store unchanged, `show_open_panel_sync` called exactly once (no internal loop) |
| T2.13 | authorize_mount: happy path            | Stub panel returns requested dir, `create_bookmark` returns data → store contains new entry keyed by canonical path |

The `MacosApi` trait should wrap the five functions in `macos.rs` so the
logic module can be tested with a fake implementation. `macos.rs` has
exactly one implementation; `stub.rs` has exactly one. No production
code branches on the trait — it's purely for testability.

**macOS integration test** (behind `#[cfg(target_os = "macos")]` and
`#[ignore]` by default; run manually):

| # | Test                                   | Assertion                                                                              |
|---|----------------------------------------|----------------------------------------------------------------------------------------|
| T2.14 | `create_bookmark` + `resolve_bookmark` | Given a real existing directory, create → resolve round-trips; resolved path equals input |
| T2.15 | `nsworkspace_open` on a text file    | Returns true; a visible side effect (default editor launches) confirms wiring manually  |

---

## Phase 3 — Browse UI: JS handler + three-way error UX

### Files touched

- `mosaicfs-server/templates/browse_list.html` — replace `hx-post`
  attributes with `onclick`
- `mosaicfs-server/templates/browse_app.html` — include the new asset
- `mosaicfs-server/assets/browse_open.js` — new
- `mosaicfs-server/src/ui/mod.rs::serve_asset` — add the new asset entry

### Steps

1. **Create `mosaicfs-server/assets/browse_open.js`.** Vanilla ES, no
   framework. No build step. Self-contained (no imports). Expose one
   global function `browseOpen(virtualPath)` and wire up click delegation
   on `document.addEventListener('click', …)` scoped to
   `[data-browse-open]` elements. The handler:
   - POSTs `/ui/browse/open` with form-encoded `path=<virtualPath>`.
   - On non-`application/json` response: generic flash.
   - On HTTP 200: extract `{ node_id, local_mount_path, relative_path }`;
     if `window.__TAURI__` is undefined, show "This file can only be
     opened from the MosaicFS desktop app." flash; otherwise call
     `window.__TAURI__.core.invoke('open_file', { target: { … } })`.
   - Tauri invoke resolves → clear flash.
   - Tauri invoke rejects with `{ code, …fields }`: route to
     `renderFlash(code, fields)`.
   - HTTP 4xx/5xx → parse JSON `{ code, message, …fields }`, route to
     `renderFlash(code, fields)`.
   - `renderFlash` builds the flash HTML for each known `code` per the
     table below; unknown codes fall through to generic red-box
     `{message}`.

2. **Flash rendering** — replace `#flash` inner HTML with the generated
   fragment. Buttons use `onclick="browseAuthorize(...)"` or
   `onclick="browseRetry(...)"`:

   | `code`                     | Message                                                                                            | Button                       |
   |----------------------------|----------------------------------------------------------------------------------------------------|------------------------------|
   | `no_host_mount`            | `No mount configured for node <code>{node_id}</code> on this host. Add a network mount in the admin UI.` | — |
   | `bookmark_not_authorized`  | `MosaicFS needs permission to open files under <code>{local_mount_path}</code> (node <code>{node_id}</code>).` | "Authorize…" → `authorize_mount(local_mount_path)` then retry `open_file` |
   | `path_not_accessible`      | `File not reachable: <code>{relative_path}</code> is not at <code>{local_mount_path}</code>. The share may be disconnected.` | "Retry" → re-invoke `open_file` with the same target |
   | `path_traversal`           | `Refusing to open <code>{requested_path}</code>: a symlink resolved to <code>{resolved_path}</code>, outside <code>{local_mount_path}</code>.` | — |
   | `mismatched_selection`     | `You selected <code>{got}</code>, but MosaicFS needs permission for <code>{expected}</code>.` | "Try again" → re-invoke `authorize_mount(expected)`. Retry is **manual** — do not loop automatically. |
   | `user_cancelled`           | Silent — clear any existing flash. User explicitly dismissed the picker; no need to nag. | — |
   | `bookmark_creation_failed` | `Couldn't save permission for that folder: {message}` | — |
   | `not_found`                | Generic "File not found." | — |
   | `open_failed` / other      | `message` verbatim, escaped | — |

   All user-supplied strings (paths, messages) must be HTML-escaped
   before injection. Implement a small `escapeHtml(s)` helper.

3. **Update `browse_app.html`** — add `<script src="/ui/assets/
   browse_open.js" defer></script>` in the `<head>` alongside the
   existing htmx script.

4. **Update `browse_list.html`** (lines 50–58):
   Replace:
   ```html
   <span class="file-name"
         hx-post="/ui/browse/open"
         hx-trigger="click"
         hx-target="#flash"
         hx-swap="innerHTML"
         hx-vals='{"path":"{{ row.virtual_path }}"}'>
     {{ row.name }}
   </span>
   ```
   With:
   ```html
   <span class="file-name"
         data-browse-open
         data-virtual-path="{{ row.virtual_path }}">
     {{ row.name }}
   </span>
   ```

5. **Register the asset in `serve_asset`** (`ui/mod.rs:357` block):
   ```rust
   "browse_open.js" => (
       include_bytes!("../../assets/browse_open.js"),
       "application/javascript; charset=utf-8",
   ),
   ```

### Unit tests (Phase 3)

JS isn't currently tested in this project. Two options:

**A. Pure-JS unit tests with a minimal runner.** Add a
`mosaicfs-server/assets/browse_open.test.html` that can be opened in a
browser and exercises `renderFlash`, `escapeHtml`, and the fetch
branching with `fetch` stubbed. Run manually. Not in CI. This is the
recommended path — no npm, no JSDOM, consistent with the project
toolchain.

Test cases to cover in the runner:

| # | Test                                      | Assertion                                                                                 |
|---|-------------------------------------------|-------------------------------------------------------------------------------------------|
| T3.1 | `escapeHtml` cases                       | `<script>` → `&lt;script&gt;`; handles `&`, `"`, `'`                                      |
| T3.2 | `renderFlash('no_host_mount', {node_id:"abc"})` | Output contains `abc`, no button element                                                  |
| T3.3 | `renderFlash('bookmark_not_authorized', {local_mount_path:"/Volumes/NAS", node_id:"x"})` | Output contains an "Authorize…" button calling `browseAuthorize`                          |
| T3.4 | `renderFlash` unknown code               | Falls back to generic; message escaped                                                    |
| T3.5 | `renderFlash('path_traversal', {…})` with path containing `<` | Path is HTML-escaped in output                                                             |
| T3.5a | `renderFlash('mismatched_selection', {expected:"/Volumes/NAS", got:"/Users/x"})` | Output contains both paths escaped; "Try again" button wired to `browseAuthorize('/Volumes/NAS')` |
| T3.6 | `browseOpen` when `window.__TAURI__` is undefined | Calls fetch, receives 200 + target JSON → shows "desktop app required" flash (without calling any Tauri function; wasn't defined) |

**B. Rust-side integration tests.** An `axum`-level test that exercises
the template rendering + asset serving: verify that `GET /ui/assets/
browse_open.js` returns `200` with `content-type:
application/javascript`, and that `GET /ui/browse` rendered HTML
contains `data-browse-open`. Place in
`mosaicfs-server/src/ui/mod.rs` tests or a new integration test file.

| # | Test                                   | Assertion                                                                         |
|---|----------------------------------------|-----------------------------------------------------------------------------------|
| T3.7 | `GET /ui/assets/browse_open.js`       | 200, body non-empty, content-type javascript                                       |
| T3.8 | Rendered `browse_list.html`          | A file row contains `data-browse-open` and `data-virtual-path`, no `hx-post` attribute on file-name spans |

---

## Cross-phase acceptance (run after all three phases land)

Manual, on macOS, from the built `.app`:

1. Configure a `network_mount` on this host's node pointing at a mounted
   NAS directory. Browse to a file. Click it.
   - **First click:** "Authorize…" flash. Clicking the button shows
     `NSOpenPanel` pre-filled to the mountpoint. Selecting the mountpoint
     and confirming opens the file in its native app.
2. Quit and relaunch the `.app`. Click the same file.
   - **Expected:** opens directly, no prompt. Bookmark persisted.
3. Unmount the NAS (Finder → eject). Click the file.
   - **Expected:** "File not reachable" flash with Retry button.
4. Re-mount the NAS. Click Retry.
   - **Expected:** opens.
5. Create a file in the node's `network_mount` that's actually a symlink
   escaping the mount (e.g. `ln -s /etc/hosts outside-link.txt` inside
   the mount). Click it.
   - **Expected:** "Refusing to open …: a symlink resolved to /etc/hosts,
     outside …" flash.
6. Delete the `network_mount` row for this host for the relevant remote
   node. Click any file from that node.
   - **Expected:** "No mount configured for node X on this host" flash,
     no button.
7. Browse to `http://localhost:8443/ui/browse` directly in Safari (not
   through the Tauri app). Click a file.
   - **Expected:** "This file can only be opened from the MosaicFS
     desktop app." flash.
8. Fresh install. Click a file whose mount needs authorization. When the
   picker opens, navigate to and select a *different* directory than the
   prompt requested. Click OK.
   - **Expected:** "You selected {got}, but MosaicFS needs permission for
     {expected}" flash with a "Try again" button. Clicking the button
     re-opens the picker (no automatic re-prompt).

---

## Coding conventions reminders (from project memory / CLAUDE.md)

- After any code changes on macOS, run `cargo build && ./scripts/
  start-dev-environment` automatically — do not ask.
- Commit only when the developer asks.
- No emojis in generated files or messages.
- Standalone `.js` under `mosaicfs-server/assets/` is fine; inline
  `<script>` only for small page-specific glue.
- Avoid new crates / abstractions if existing ones serve the need. The
  `objc2-*` additions here are justified by the lack of a sandboxed-
  friendly alternative; the `base64` dep is small and already in most
  Rust trees via transitive deps (check the workspace lockfile before
  adding).
