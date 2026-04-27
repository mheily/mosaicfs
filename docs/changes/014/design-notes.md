# Change 014: Design notes for Phase 2

These notes pin down the decisions the architecture doc deliberately left
implicit. Phase 1 (the `WatchPathProvider` trait and `start_agent` signature
change) needs no extra design — it is a mechanical refactor with one
production call site (`mosaicfs-server`'s agent startup, currently spawning
`start_agent(cfg, secrets)`).

## 1. Canonical-vs-raw paths

**Rule:** `settings.watch_paths` and `BookmarkStore` keys are both the
canonical path string, always. The user-typed string never round-trips.

- `add_watch_path` runs `NSOpenPanel`, takes the user's selection, calls
  `std::fs::canonicalize`, and uses the resulting `PathBuf::display()`
  string for both the settings entry and the bookmark store key. If
  canonicalize fails, return an error to the UI; do not fall back to the
  raw selection (a path that won't canonicalize won't crawl either).
- `authorize_watch_path(path)` treats the incoming `path` as already
  canonical (it came from `list_watch_paths`). It pre-selects that path in
  `NSOpenPanel`, then runs the same canonicalize-and-compare check
  `authorize_mount_inner` already does (`commands.rs:171-176`).
- `remove_watch_path(path)` removes the entry from settings whose string
  equals `path` exactly, and removes the bookmark with the same key. No
  fuzzy matching.
- The `BookmarkedWatchPathProvider` uses each settings entry directly as
  the bookmark store key — no re-canonicalization at provider time. If
  the underlying directory was moved or renamed, the bookmark either
  resolves to the new location (bookmarks track the inode, not the path)
  or returns `Stale`; both cases are handled in §2.

## 2. Reconciliation invariants

The UI shows exactly what's in `settings.watch_paths`. The bookmark store
is queried per-row to compute the badge. Three abnormal states are
possible; each has a defined behavior:

| State | UI shows | Crawler behavior |
|---|---|---|
| Settings entry, no bookmark | "Needs authorization" badge + Authorize… button | Skipped with a `tracing::warn!`; no error |
| Settings entry, bookmark resolves to a different path (moved dir) | "Authorized" badge | Crawls the *resolved* path, not the settings string. Logged at `info` on first crawl after launch |
| Settings entry, bookmark is `Stale` | "Needs authorization" badge | Skipped; the stale bookmark is **removed** from the store on first failed resolve, mirroring `commands.rs:121-128` |
| Bookmark store entry with no matching settings path | (not shown) | Ignored. Orphans accumulate at most one per removed path; not worth a sweep |

Orphaned bookmarks are tolerable: the store is small, there's no security
implication, and a sweep adds code for no user-visible benefit. If this
ever matters, add a one-line "remove unreferenced bookmarks" pass to
`remove_watch_path`. Don't add it preemptively.

## 3. Add-flow ordering

`add_watch_path` performs three side effects: open the panel, write the
bookmark, write settings. Order:

1. Show `NSOpenPanel`. If cancelled → return `UserCancelled`, no state
   changes.
2. Canonicalize selection. If it fails → return an error, no state
   changes.
3. Call `create_bookmark` and insert into `BookmarkStore`. If this fails →
   return error, no state changes (settings still untouched).
4. Append the canonical path to `settings.watch_paths` and call
   `settings::save`. If this fails → log loudly, attempt to roll back the
   bookmark insert (`store.remove(...)`), return error.

Rationale: the bookmark write is the operation that can fail for
sandbox/IO reasons the user can act on; do it first so a settings entry
never exists without its bookmark. Step 4's rollback covers the unlikely
case where the bookmark succeeds but the settings file write fails (disk
full, app data dir gone). Don't worry about a crash between 3 and 4 — it
leaves an orphan bookmark, which §2 already tolerates.

`remove_watch_path` runs in the opposite order: settings first, then
bookmark. If the bookmark removal fails after settings is written, the
orphan is again tolerated.

## 4. `agent.html` visual conventions

Match `setup.html` (`desktop/ui/setup.html`) exactly — same CSS variables,
same button styling, same `-apple-system` font stack, same
`#f5f5f7` background. Don't introduce a framework, a CSS file, or a
build step. The two files are siblings and should look like they came
from the same designer.

Layout:

```
┌─────────────────────────────────────────┐
│ Watch Folders                           │  ← h2, same style as setup
│ Folders MosaicFS will index on this Mac.│  ← p.subtitle
│                                          │
│ ┌─────────────────────────────────────┐ │
│ │ /Users/me/Documents      ✓ Authorized│ │  ← row, no buttons
│ │                                  Remove │ │
│ ├─────────────────────────────────────┤ │
│ │ /Users/me/Photos       ⚠ Needs auth │ │
│ │                       Authorize… Remove │ │
│ └─────────────────────────────────────┘ │
│                                          │
│              [ + Add Folder… ]           │  ← primary button
│                                          │
│ ⓘ Restart MosaicFS to apply changes.    │  ← shown only after edits
└─────────────────────────────────────────┘
```

Window size: 480 × 380, resizable. Larger than `setup` because the row
list grows with the user's folders.

The "Restart…" banner appears after any successful add/authorize/remove
and persists until the window is reopened. It's a static element, no
auto-restart prompt. The user closes the window and quits via the tray.

JS structure: one `<script>` block that defines four async functions
calling the four Tauri commands via `window.__TAURI__.core.invoke`. After
each successful call, re-render by calling `list_watch_paths()` and
rebuilding the row list in-place. No framework, no virtual DOM — the
folder count is small enough that a full innerHTML replace is fine.
