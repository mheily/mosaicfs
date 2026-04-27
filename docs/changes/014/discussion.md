# Change 014: Embed the agent in the desktop app

## Goal

Run the mosaicfs crawler (agent subsystem) inside the desktop app process, the
same way the web UI server is already embedded. The user should be able to have
a single running app that handles both the UI and the file indexing for the
local machine.

## What was done

- Added `mosaicfs-agent` as a dependency of the desktop crate.
- Added `watch_paths` and `excluded_paths` fields to `Settings` / `settings.json`.
- Created `desktop/src/agent.rs`: builds a `MosaicfsConfig` from settings and
  spawns `mosaicfs_agent::start_agent` as a background tokio task on startup.
- Modified `server::build_router` to return `(Router, Option<String>)` so the
  resolved `node_id` can be shared with the agent — both subsystems use the
  same node identity.
- Agent does not start when `watch_paths` is empty, so existing installs are
  unaffected until the user opts in.

## Configuring watch_paths (no UI yet)

Edit `settings.json` directly. On macOS the app runs in a sandbox, so the
actual path is inside the container:

```
~/Library/Containers/com.mosaicfs.desktop/Data/Library/Application Support/com.mosaicfs.desktop/settings.json
```

The CouchDB fields are already present from the Connection setup window. You
only need to add `watch_paths` and optionally `excluded_paths`:

```json
{
  "couchdb_url": "...",
  "couchdb_user": "...",
  "couchdb_password": "...",
  "watch_paths": [
    "/path/to/directory"
  ],
  "excluded_paths": []
}
```

After editing, **restart the app** for changes to take effect (see known
limitations below).

The agent state (SQLite database, node_id file) is stored alongside settings:

```
~/Library/Containers/com.mosaicfs.desktop/Data/Library/Application Support/com.mosaicfs.desktop/agent-state/
```

---

## Known limitations / items to account for in future changes

### 1. macOS App Sandbox blocks access to watch_paths (blocker)

The desktop app is sandboxed (`com.apple.security.app-sandbox`). The only
filesystem entitlements it carries are `files.user-selected.read-only` (files
the user picks via NSOpenPanel) and `files.bookmarks.app-scope`
(security-scoped bookmarks). Any path configured in `watch_paths` that the
user has not explicitly opened via NSOpenPanel will be silently denied by the
kernel — the crawler never sees a permission error, it just cannot access the
directory at all.

This is a blocker. The agent is structurally wired up correctly but cannot
crawl anything useful until this is solved.

**The sandbox stays. The solution is security-scoped bookmarks per watch_path.**

This is the same pattern already used for VFS mount-point authorization
(`commands::authorize_mount`). The flow for watch_paths would be:

1. User adds a path to `watch_paths` in settings (via UI, not by editing JSON).
2. App detects the path has no bookmark yet and prompts the user to authorize
   access via NSOpenPanel, pre-selected at the configured path.
3. User confirms in the panel.
4. App calls `create_bookmark` and stores the result in the existing
   `BookmarkStore` (same store used for VFS mount authorization — the data
   structure is identical and the key spaces don't collide in practice).
5. On each crawl cycle, the crawler resolves the bookmark, calls
   `startAccessingSecurityScopedResource`, walks the tree, then calls
   `stopAccessingSecurityScopedResource`.

This means the `watch_paths` strings in `settings.json` are not sufficient on
their own — each path also needs a stored bookmark before the crawler can
access it. A path with no bookmark is skipped (with a logged warning) rather
than attempted and silently denied.

Implementation work required:
- Crawler changes: accept resolved `(path, bookmark_data)` pairs instead of
  bare `PathBuf`s, and call `startAccessingSecurityScopedResource` /
  `stopAccessingSecurityScopedResource` around each walk.
- UI: agent settings page with per-path authorize buttons (NSOpenPanel flow).
  This makes limitation 3 (no UI) a prerequisite, not just a nice-to-have.
- The `watch_paths` field in `Settings` / `settings.json` stays as-is; it is
  the list of intended paths. Authorization state (bookmarks) lives in the
  existing `BookmarkStore`.

### 2. No dynamic restart when watch_paths change

`mosaicfs_agent::start_agent` contains its own `shutdown_signal()` loop that
waits for `SIGTERM` or `Ctrl-C`. There is no cancellation path exposed to the
caller. This means:

- Editing `watch_paths` while the app is running has **no effect until
  restart**.
- The `save_settings` Tauri command rebuilds the web UI router in place but
  deliberately skips restarting the agent.

**To fix later:** refactor `start_agent` to accept a
`tokio_util::sync::CancellationToken` so the desktop can cancel and respawn it
after a settings change. Low priority until the sandbox problem is solved and
a UI exists for editing watch_paths.

### 3. No UI for configuring watch_paths (prerequisite for limitation 1)

The sandbox fix requires an NSOpenPanel authorization step per watch_path,
which means there must be a UI — there is no way to pre-authorize paths by
editing JSON. The "Connection…" setup window only covers CouchDB credentials
and is the wrong place for this.

**Required work:** an agent settings page that lists configured watch_paths,
shows which ones are authorized (bookmark present) vs. pending, and provides
an "Authorize…" button per path that opens NSOpenPanel and stores the
resulting bookmark. Adding or removing paths must also go through this UI
so that the bookmark store stays consistent with `watch_paths` in settings.

This must be built before the agent is functional. The JSON-editing workaround
documented above does not work in practice because the bookmarks cannot be
created without NSOpenPanel.
