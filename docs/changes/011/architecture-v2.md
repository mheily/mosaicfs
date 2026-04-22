# Change 011: Secure File Access via Existing NAS Mount + Security-Scoped Bookmarks

_This is a full rewrite of `architecture.md` after scope change 1 (see
`scope_change-1.md`). It stands on its own; `architecture.md` is superseded._

---

## Current State Summary

_Condensed from the inventory auto-generated at abfe84e. Supplemented by reads
of the files this change touches._

**Workspace crates relevant to this change:**

- `mosaicfs-server` (9,225 lines) — Axum server. Hosts `/api/*` and `/ui/*`
  routes on HTTP port 8443.
- `mosaicfs-common` — shared documents, including the `NetworkMount` schema
  that carries the laptop's NAS-mount information.
- `desktop/` — Tauri shell. `desktop/src/lib.rs` opens a single
  `WebviewWindowBuilder` pointed at `http://localhost:8443/ui/browse`. No
  plugins, no custom Rust beyond `lib.rs`. `Cargo.toml` has only
  `tauri = { version = "2", features = [] }`. Entitlements
  (`Entitlements.plist`): `app-sandbox` + `network.client` only — no
  filesystem, no user-selected files, no process execution.

**`NetworkMount` schema** (`mosaicfs-common/src/documents.rs:231-241`):

```rust
pub struct NetworkMount {
    pub mount_id: String,
    pub filesystem_id: String,
    pub remote_node_id: String,
    pub remote_base_export_path: String,
    pub local_mount_path: String,
    pub mount_type: String,
    pub priority: i32,
}
```

Carried on `NodeDocument.network_mounts: Option<Vec<NetworkMount>>`
(`documents.rs:195`). The document describes, for this node, where remote
nodes' exported directories have been mounted locally. Populated by
out-of-band means (admin config, Finder-initiated mount, fstab, etc.).

**Files directly touched by this change:**

- `mosaicfs-server/src/ui/open.rs` — `open_file_by_id` (lines 32–124) today
  resolves `file_id → source.node_id + source.export_path → network_mounts →
  local_path`, checks the path exists, then spawns `open` / `xdg-open`. The
  spawn is the security problem. Lines 106–123 (spawn + existence check) and
  `summarize_open_error` (126–138) go away; the resolver stays in a new shape.
- `mosaicfs-server/src/ui/browse.rs` (lines 211–230) — the `POST
  /ui/browse/open` handler currently calls `open_file_by_id` and returns a
  flash fragment. It becomes a JSON endpoint.
- `mosaicfs-server/src/ui/actions.rs:22` — imports `open_file_by_id` but has
  zero call sites (dead import, confirmed by grep). Removed.
- `mosaicfs-server/templates/browse_list.html:52-56` — the `<span class=
  "file-name">` currently does `hx-post="/ui/browse/open"` and swaps a flash
  fragment into `#flash`. It becomes a JS click handler that calls the Tauri
  command.
- `desktop/src/lib.rs` — grows a Tauri command `open_file` and a bookmark
  store, injected into the webview via `.invoke_handler`.
- `desktop/Cargo.toml` — adds `objc2` + `objc2-foundation` +
  `objc2-app-kit` (for `NSOpenPanel`, `NSWorkspace`, `NSURL` bookmark APIs)
  and `serde` for the command's request/response types. `tauri-plugin-shell`
  is **not** added — `NSWorkspace.openURL` is a direct Cocoa call.
- `desktop/Entitlements.plist` — replaces the current minimal set with
  `user-selected.read-only` + `bookmarks.app-scope`.
- `desktop/capabilities/default.json` — whitelists the new command.

**Deployment today:** two-container pod (`couchdb` + `mosaicfs`) built from
`Dockerfile.mosaicfs`. This change does not alter the pod composition. The
Tauri app ships separately as a macOS `.app` bundle; not a container.

**External services:** CouchDB (unchanged), S3 replication (unchanged).

**docs/changes/012:** does not exist (confirmed by `ls docs/changes/`). The
deletion instruction from the prior architecture.md is moot.

---

## Goal

Remove the server's ability to spawn `open` / `xdg-open` on
attacker-controllable paths by moving the open action to the sandboxed Tauri
app. The server returns a structured path descriptor; the Tauri app joins it
against a user-authorized mountpoint (held as a macOS security-scoped
bookmark) and calls `NSWorkspace.openURL`.

No WebDAV, no mount management, no LaunchAgent, no separate WebDAV
credential. The NAS is mounted by whatever mechanism the user already uses
(Finder, fstab, autofs) and surfaced via the existing `network_mounts`
field.

---

## Changes

### Change 1: Server returns a structured descriptor, not a command

**Today:** `ui/open.rs::open_file_by_id` does the full `file_id → local_path`
resolution and then calls `std::process::Command::new("open"|"xdg-open")` on
the result (`open.rs:106-115`). The server process — running as a separate
build account specifically for blast-radius reasons — thereby invokes the
user's LaunchServices on whatever path a `file_id` happens to resolve to.

**Proposed:** Rewrite `open_file_by_id` to return a descriptor:

```rust
pub struct OpenTarget {
    pub node_id: String,          // source node (for diagnostics/UI)
    pub local_mount_path: String, // directory the Tauri app must hold a bookmark for
    pub relative_path: String,    // joined onto local_mount_path by the caller
}
```

Resolution preserves the existing `network_mounts` match (longest
`remote_base_export_path` prefix, then highest `priority`, per
`open.rs:77-94`). For a matched mount, `local_mount_path` is the mount's
`local_mount_path` and `relative_path = source.export_path -
remote_base_export_path`. The resolver returns `local_mount_path` verbatim
from the document — **canonicalization is done on write, not on read** (see
Change 1a), so the Tauri app can use the string directly as a bookmark-store
key without renormalizing. The existence check (`open.rs:102-104`), the
subprocess spawn, and `summarize_open_error` are all deleted. Error variants
contract to the surface the caller actually needs: `NotFound(file_id)`,
`NoNodeId`, `NodeNotRegistered`, `NoHostMount(source_node_id)`. No
`SpawnFailed`, no `PathNotAccessible` — those failure modes now live in the
Tauri app.

**Same-node source** (`state.node_id == source.node_id`): the file lives on
this node's real filesystem at `source.export_path`; no `network_mount` is
involved. Resolution uses the source node's own `storage[]` entries
(`NodeDocument.storage: Option<Vec<StorageEntry>>`, each with a
`mount_point`). Find the entries whose `mount_point` is a path-prefix of
`source.export_path`, pick the **longest match** (mountpoints can nest), and
return that `mount_point` as `local_mount_path` with `relative_path =
source.export_path - mount_point` (leading slash stripped; see below). If no
entry matches — source export path is not under any known filesystem on the
source node — return `NoHostMount(source_node_id)`; the remediation is the
same class (admin must register the filesystem), so sharing the variant
keeps the UI surface small.

**Behavior change from current code:** today `open.rs:56` returns
`source.export_path` verbatim for same-node files and skips any `storage[]`
walk. That was safe because the server spawned `open` itself. With
bookmarks on the Tauri side, a same-node open now requires the source node
to have populated `storage[]` — falling back to `local_mount_path: "/"`
would force the user to authorize their whole filesystem, which defeats the
sandbox. In practice the agent populates `storage[]` before files get
indexed, so this fires mainly in bootstrap states.

**Wire contract: `relative_path` never begins with `/`.** Both resolution
paths above produce a raw prefix-strip (e.g. `/export/photos/a.jpg -
/export = /photos/a.jpg`); the resolver strips the leading `/` before
returning. The Tauri app joins with `PathBuf::join`, which treats a
leading-`/` component as absolute and silently drops the mountpoint —
exactly the mistake that would break the traversal check. Making the wire
shape unambiguous removes that footgun at the boundary.

**Justification:** The server's process-spawn on behalf of the main user
account is exactly the security boundary the separate build account is meant
to enforce. Returning a descriptor moves the open decision (and the
filesystem capability that enables it) to a principal the user's session
already trusts with their files.

### Change 1a: Canonicalize `local_mount_path` on write

**Today:** `handlers/nodes.rs::add_mount` (lines 223–260) writes
`body.local_mount_path` verbatim into the node document. `patch_mount`
similarly passes through the JSON. The same lack of normalization applies
to `storage[].mount_point` populated by the agent (`storage[]` is
crawled state, generally already canonical, but we should not rely on that
implicitly when the Tauri app keys bookmarks by string equality).

**Proposed:** `add_mount` and `patch_mount` normalize `local_mount_path`
before writing: trim trailing slashes (except root), collapse `//`, and
call `std::fs::canonicalize` when the path exists on the host running the
handler — falling back to the normalized-but-unresolved form when it does
not (cross-host admin writes, or the mount is not currently active). The
agent's storage-crawl path does the same normalization on `mount_point`
when populating the node document. The Tauri app treats the string it
receives as canonical and does not re-normalize for bookmark lookup.

**Justification:** The bookmark store is keyed by `local_mount_path`
string. If an admin types `/Volumes/NAS/` and the agent writes
`/Volumes/NAS`, the Tauri app sees them as two different mountpoints and
prompts for authorization twice. Canonicalize once, at the write boundary,
and every reader downstream can trust the form. `std::fs::canonicalize`
during the HTTP write is a minor cost (a couple of `lstat` calls) and only
runs when the admin actually edits mounts.

**Caveat:** `std::fs::canonicalize` resolves symlinks. If an admin
configures the mount via a symlink on purpose, the canonical form written
to the document will be the target, not the symlink. This is the desired
behavior — it matches what the Tauri app will canonicalize to when it
opens the file — but it's worth noting in the admin UI so the admin isn't
surprised that the field value changed on save.

### Change 2: Browse-open handler returns JSON

**Today:** `ui/browse.rs::open` (lines 211–230) returns a flash HTML
fragment via `hx-swap` targeting `#flash`. On success it says `"Opened
{path}"`; on failure it shows the error string. The side effect (the actual
open) happens inside the handler.

**Proposed:** The handler does no side effect. It resolves the virtual path
to a `file_id`, calls the new resolver, and returns:

- `200` with `Content-Type: application/json` and the `OpenTarget` body on
  success
- `404` / `409` / `500` with a JSON `{ "code": "...", "message": "..." }`
  body on resolver errors (one status per error variant so the client can
  branch)

HTMX swapping is removed from this action; the template switch to a JS click
handler is Change 3.

**Justification:** The Tauri app needs the descriptor as data, not as
rendered HTML. Keeping the handler JSON-only keeps the server from caring
whether the caller is HTMX, Tauri, or a CLI tool.

### Change 3: Browse template invokes the Tauri command

**Today:** `templates/browse_list.html:50-58` — the file-name span has
`hx-post="/ui/browse/open"` and swaps the response into `#flash`. A browser
user outside the Tauri webview sees the flash; the server spawns the open.

**Proposed:** Replace the `hx-post` attributes with a plain `onclick`
handler that:

1. Fetches `POST /ui/browse/open` as JSON.
2. On `200`: calls Tauri's `window.__TAURI__.core.invoke('open_file',
   target)` with the descriptor as the argument.
3. On non-`200` or on Tauri command errors: writes a flash into `#flash` via
   the existing flash slot in `browse_app.html`.
4. If `window.__TAURI__` is undefined (user browsed to `/ui/browse`
   directly in Safari, not through the Tauri shell): shows a flash
   explaining the desktop app is required.

The JS lives in a standalone asset, `mosaicfs-server/templates/assets/
browse_open.js`, included from `browse_app.html` via a `<script>` tag
alongside the existing `htmx.min.js` / `pico.min.css`. Same embedding
mechanism as those — no bundler, no npm, just a served static file. The
"no separate JS build" convention is a warning against pulling in
framework toolchains (webpack, npm, Angular), not against putting
handwritten JS in its own file.

**Justification:** The Tauri shell is the principal that's authorized (via
the bookmark) to join paths and call `NSWorkspace.openURL`. The server can't
do it; HTMX can't do it; a JS handler inside the Tauri webview can.

### Change 4: Tauri `open_file` command with security-scoped bookmarks

**Today:** `desktop/src/lib.rs` is a 15-line `WebviewWindowBuilder`. No
plugins, no commands, no bookmark store.

**Proposed:** Add a single Tauri command:

```rust
#[tauri::command]
async fn open_file(target: OpenTarget) -> Result<(), OpenError>

struct OpenTarget {
    node_id: String,
    local_mount_path: String,
    relative_path: String,
}

enum OpenError {
    BookmarkNotAuthorized {             // Tauri has no bookmark yet for this
        local_mount_path: String,       //   mount; UI should call `authorize_mount`
        node_id: String,
    },
    PathNotAccessible {                 // bookmark exists but file isn't reachable
        local_mount_path: String,       //   (out-of-band unmount, file moved,
        relative_path: String,          //    permission revoked in System Settings)
    },
    PathTraversal {                     // canonicalized path escaped the mountpoint.
        requested_path: String,         //   In practice: a symlink component inside
        resolved_path: String,          //   the mount pointed outside it. Both forms
        local_mount_path: String,       //   are surfaced so the user can see e.g.
    },                                  //   "'bar' in /mnt/nas/foo/bar/baz resolved
                                        //    to /mnt/other/baz".
    OpenFailed(String),                 // NSWorkspace.openURL returned false
}
```

**Flow (macOS):**

1. Canonicalize `local_mount_path` (`std::fs::canonicalize`) — resolves the
   `/private/var/folders/…` shadow form macOS sometimes exposes for volumes.
2. Look up a bookmark for the canonical mountpoint in the app's bookmark
   store. If absent, return `BookmarkNotAuthorized`. **No picker is shown
   from `open_file` itself** — the UI surfaces the error distinctly (see
   Change 5), the user invokes a separate `authorize_mount` command.
3. Resolve the bookmark (`NSURL(resolvingBookmarkData:options:.withSecurityScope,
   …, isStale:&stale)`). If `isStale`, treat as `BookmarkNotAuthorized` and
   remove the stored bookmark so the user gets a fresh picker.
4. `url.startAccessingSecurityScopedResource()`. Deferred
   `stopAccessingSecurityScopedResource()` via RAII wrapper.
5. Let `requested_path = canonical(local_mount_path) / relative_path`.
   Canonicalize it (`std::fs::canonicalize`, which resolves symlinks). Call
   the result `resolved_path`. If `resolved_path` is not inside
   `canonical(local_mount_path)`, return `PathTraversal { requested_path,
   resolved_path, local_mount_path }`. The realistic trigger is a symlink
   component inside the mount pointing outside it (e.g. `bar` in
   `/mnt/nas/foo/bar/baz` is a symlink to `/mnt/other/baz`); returning both
   paths lets the UI surface exactly what the OS resolved the request into.
6. Verify the joined path exists. If not, return `PathNotAccessible`.
7. Build `NSURL(fileURLWithPath:)` and call `NSWorkspace.sharedWorkspace()
   .openURL(url)`. If it returns `false`, return
   `OpenFailed("LaunchServices refused to open the file")`.

A second command:

```rust
#[tauri::command]
async fn authorize_mount(local_mount_path: String) -> Result<(), AuthorizeError>
```

Shows `NSOpenPanel` with `canChooseDirectories = true`, `canChooseFiles =
false`, `directoryURL` pre-filled to `local_mount_path`, and the prompt
copy "Authorize MosaicFS to open files from this mountpoint." If the user's
selection canonicalizes to the requested mountpoint, serialize a
security-scoped bookmark (`NSURL.bookmarkData(options:.withSecurityScope,
…)`) and persist it. If the selection doesn't match, return
`MismatchedSelection { expected, got }` — one try per command invocation,
no auto-loop. The UI renders a flash naming both paths and a button to try
again; the user clicks to re-invoke.

**Bookmark storage:** a single JSON file in the app's data directory
(`tauri::path::BaseDirectory::AppData`), shaped as a versioned envelope:

```json
{
  "version": 1,
  "bookmarks": {
    "/Volumes/NAS": "<base64 bookmark data>",
    "/Volumes/Archive": "<base64 bookmark data>"
  }
}
```

The key is the canonicalized `local_mount_path`; the value is base64 of the
`NSData` returned by `bookmarkData(options:.withSecurityScope, …)`.
Multiple authorized mountpoints coexist in a single `bookmarks` map; each
call to `open_file` looks up by the specific `local_mount_path` from the
server's descriptor. No cross-mountpoint "merging" — one bookmark per
mountpoint. On load, an unknown `version` is treated as an empty store (log
a warning and re-prompt for authorization as needed) rather than hard-
failing, so a future format change doesn't brick older installs.

**Entitlements (`Entitlements.plist`):**

```xml
<key>com.apple.security.app-sandbox</key><true/>
<key>com.apple.security.network.client</key><true/>
<key>com.apple.security.files.user-selected.read-only</key><true/>
<key>com.apple.security.files.bookmarks.app-scope</key><true/>
```

No `files.user-selected.read-write` (the system is read-only permanently,
per the decisions doc). No temporary-exception paths. No
`inherit.user-selected` — the webview doesn't need sandbox inheritance
because the Tauri Rust process performs the bookmark resolution and the OS
opens the file under the target app's own sandbox.

**Linux/Windows:** Command returns `OpenFailed("desktop open not
implemented on this platform")` as a compile-time stub. The descriptor
shape is portable; Linux gets `xdg-open` with a flatpak-portal
authorization pattern in a later change, Windows gets `ShellExecuteW`.

**Justification:** Security-scoped bookmarks are the documented macOS
mechanism for a sandboxed app to hold persistent access to a user-selected
path whose location is not known at code-signing time. The alternatives
table in `scope_change-1.md` rules out the other candidates (hardcoded
entitlement, `sandbox_init`, nullfs, symlinks). The call sequence matches
Apple's Powerbox reference flow.

### Change 5: UI surfaces three error states distinctly

**Today:** N/A — a single flash for success or failure.

**Proposed:** The browse-open JS handler branches on the error surface from
Changes 1 and 4 and renders three distinct flashes:

| Error surface | UI treatment |
|---|---|
| Server `NoHostMount(node_id)` | Flash: "No mount configured for node `{node_id}` on this host. Add a `network_mount` entry for this node in the admin UI." No action button. |
| Tauri `BookmarkNotAuthorized { local_mount_path, node_id }` | Flash: "MosaicFS needs permission to open files under `{local_mount_path}` (node `{node_id}`)." Button: "Authorize…" → invokes `authorize_mount(local_mount_path)`, then retries `open_file`. |
| Tauri `PathNotAccessible { local_mount_path, relative_path }` | Flash: "File not reachable: `{relative_path}` is not present at `{local_mount_path}`. The network share may have been disconnected or the file may have moved." Button: "Retry" → re-invokes `open_file` with no re-authorization. |
| Tauri `PathTraversal { requested_path, resolved_path, local_mount_path }` | Flash: "Refusing to open `{requested_path}`: a symlink in the path resolved to `{resolved_path}`, which is outside the authorized mountpoint `{local_mount_path}`." No action button — this is a structural problem with the file, not something the user can retry away. |
| Tauri `MismatchedSelection { expected, got }` (from `authorize_mount`) | Flash: "You selected `{got}`, but MosaicFS needs permission for `{expected}`. Open the picker again?" Button: "Try again" → re-invokes `authorize_mount(expected)`. |

Other variants (`NotFound`, `NoNodeId`, `NodeNotRegistered`, `OpenFailed`)
fall through to a generic error flash carrying the message string. These
are bugs or misconfigurations, not user-actionable states.

**Justification:** Each of the three distinct states requires a different
recovery: admin configuration (no host mount), user authorization (no
bookmark), or waiting/reconnecting the NAS (path not accessible). Merging
them forces the user to guess which one they're in.

---

## Implementation Phases

Phases are organized by topical concern. Tree will not build between
phases; final state is what matters.

### Phase 1 — Server-side: descriptor-returning resolver

Rewrite `ui/open.rs` to export `OpenTarget` and the reduced error enum.
Rewrite `open_file_by_id` to stop at the resolver. Delete the spawn, the
existence check, and `summarize_open_error`. Change `ui/browse.rs::open` to
return JSON (with distinct HTTP statuses per error variant) instead of a
flash fragment. Remove the dead `open_file_by_id` import in
`ui/actions.rs:22`. In `handlers/nodes.rs`, normalize
`local_mount_path` inside `add_mount` and `patch_mount` (Change 1a); add
the same normalization to wherever the agent writes `storage[]
.mount_point`.

**Acceptance:** `curl -X POST /ui/browse/open -d path=...` returns an
`OpenTarget` JSON body for a valid file, or a JSON error body with the
right HTTP status for each error class. `cargo build` passes. The server
process never executes `std::process::Command` in this code path. Grep the
crate for `Command::new("open")` / `Command::new("xdg-open")` — should be
zero hits in `ui/`.

### Phase 2 — Tauri-side: `open_file` + `authorize_mount` commands

Add `objc2` deps to `desktop/Cargo.toml`. Grow `desktop/src/lib.rs` into a
small module tree (`lib.rs`, `commands.rs`, `bookmarks.rs`). Implement the
bookmark store (JSON file in app data dir), `authorize_mount` (NSOpenPanel +
bookmark creation), and `open_file` (bookmark resolve, scoped access, path
validation, `NSWorkspace.openURL`). Update `Entitlements.plist` to the four
keys above. Whitelist both commands in `capabilities/default.json`. Register
the commands with `.invoke_handler` in the builder.

**Acceptance:** Built `.app` launches; first open click produces
`BookmarkNotAuthorized`; invoking `authorize_mount` shows the picker;
selecting the mountpoint and re-clicking opens the file in its native app.
Unmounting the NAS out-of-band and clicking again produces
`PathNotAccessible`. Re-mounting and clicking works. Restarting the app
does not re-prompt (bookmark persists).

### Phase 3 — Browse template: JS handler + three-way error UX

Replace the `hx-post` on the file-name span in `browse_list.html` with a
click handler that calls `fetch('/ui/browse/open')` then dispatches to the
Tauri command. Render the three flash shapes (with buttons) per the table
in Change 5. Keep the existing flash slot in `browse_app.html`; templates
stay Tera, no JS build.

**Acceptance:** Clicking a file where the server has no `network_mount`
entry shows the "no mount configured" flash (no button). Clicking before
authorization shows the "Authorize…" flash; pressing the button shows the
picker, then the file opens. Clicking while the NAS is unmounted shows the
"File not reachable" flash with a "Retry" button.

**Cross-phase dependency:** Phase 3 needs Phase 1 (the JSON endpoint) and
Phase 2 (the Tauri commands) — without either, the click handler has
nothing to call. Phase 2 only needs Phase 1 for end-to-end exercise;
`authorize_mount` can be tested independently of the server.

---

## What Does Not Change

- **`mosaicfs-vfs` crate.** The partial FUSE implementation remains
  unwired. Not touched.
- **`mosaicfs-agent`.** No agent changes.
- **Pod composition.** Two containers (`couchdb`, `mosaicfs`). No new
  container, no `mosaicfs-smb` image, no WebDAV server.
- **REST API surface.** `/api/*` routes are unchanged. `GET
  /api/files/{id}/content` keeps its JWT auth. `/ui/browse/open` changes
  shape but stays at the same path.
- **JWT session auth.** Unchanged. No `dav_password`, no Keychain seeding,
  no Basic auth.
- **Port.** Server stays on 8443. No `/dav/*` routes.
- **TLS posture.** HTTP-only on localhost, same as today. Revisit if/when
  the server moves off the Tauri host.
- **`network_mounts` schema.** Unchanged — this change consumes the
  existing fields, adds none.
- **Tauri as a thin native shell.** The decisions doc (2026-04-05)
  replaced React-as-UI-framework with server-rendered HTMX but did not
  eliminate the Tauri bundle; the `desktop/` crate remains a webview shell
  pointed at `/ui/browse`. This change grows it with native commands but
  does not reintroduce a JS framework inside the webview.
- **Replication, CouchDB federation, agent crawlers.** Out of scope.

---

## Deferred

- **Linux / Windows native-open implementations.** Stubbed; not built. Add
  with `xdg-open` (Linux) and `ShellExecuteW` (Windows) when a non-macOS
  desktop becomes a real user.
- **XPC helper.** Earlier designs proposed an out-of-process path validator.
  The threat model (untrusted server, trusted-enough Tauri app) doesn't
  require it, and the Tauri command interface is shaped to allow slotting
  one in without touching the server or the browse UI.
- **Temp-file fallback.** If the NAS is unmounted, we surface
  `PathNotAccessible` rather than downloading and opening a copy. Open-a-copy
  is a degraded UX (edits lost) that we can add later if it's actually
  needed.
- **Full FUSE / WebDAV mount of the VFS.** The decision to drop WebDAV for
  this change does not prevent a later change from exposing the VFS as a
  real filesystem for clients that want to browse it outside the Tauri
  shell. Out of scope here.
- **Multiple Tauri-app windows / reopen-last-session state.** Not
  introduced.
- **Bookmark expiry / user-triggered re-authorization.** v1 removes a
  bookmark only on `isStale`. A "forget authorized mountpoints" UI is a
  future settings concern.
- **Automated sleep/wake remount detection.** `PathNotAccessible` with a
  retry button is the v1 recovery. If users hit this often in practice, add
  a reachability-check-then-poll loop in the Tauri app.
