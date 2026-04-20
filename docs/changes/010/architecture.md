# Change 010 — End-user file browser at `/ui/browse`

**Dependency:** this change assumes change 009 (rename `/admin` → `/ui`) has
already landed. All URLs, module names, and cookie references below reflect
the post-009 state.

## 1. Current State Summary

_Condensed from the auto-generated inventory (commit `9be6abe`), supplemented
by direct reads of the code, and projected forward through change 009._

### Relevant crates

| Crate | Role |
|-------|------|
| `mosaicfs` | The single unified binary (change 006). |
| `mosaicfs-server` | Axum-based REST API + server-rendered UI. |
| `mosaicfs-vfs` | Shared readdir/mount evaluation used by FUSE and the server. |
| `mosaicfs-common` | Document schemas, step definitions. |

### Existing UI surface (after change 009)

The server-rendered UI lives under `/ui/*`, implemented in
`mosaicfs-server/src/ui/`:

- Router: `mosaicfs-server/src/ui/mod.rs`
- Views (GET handlers, page renders): `mosaicfs-server/src/ui/views.rs`
- Actions (POST handlers, form submits): `mosaicfs-server/src/ui/actions.rs`
- Templates: `mosaicfs-server/templates/*.html` (Tera), 20 files
- Session auth via `tower-sessions` (cookie `mosaicfs_session`, 12-hour idle)
- Assets: `pico.min.css` + `htmx.min.js` embedded via `include_bytes!`, served from `/ui/assets/{path}`
- `require_auth` middleware gates the `protected` sub-router
- `MOSAICFS_INSECURE_HTTP=1` bypasses auth for local development

### The `/ui/browse` page today (post-009)

`ui::views::browse_page` renders `templates/browse.html`. It is a
directory-centric page that mixes file listing with admin affordances:

- Breadcrumb nav
- "Create a directory here" button → `/ui/vfs/new?parent=…`
- "Edit this directory" / "Delete this directory" buttons → `/ui/vfs/dir?path=…`
- Mount count summary
- Subdirectories table (click-through navigation)
- Files table with columns: name, source node, mime type, labels, `Open` button

### Where VFS configuration lives

All VFS-directory CRUD already has its own home at `/ui/vfs/*`:

- `GET /ui/vfs` — list of all VFS directories
- `GET /ui/vfs/new` — create form
- `GET /ui/vfs/dir?path=…` — edit/configure/delete one directory, including mounts and steps

This means the admin affordances currently co-located on `/ui/browse` are
duplicates of flows already reachable from `/ui/vfs`. Removing them from the
browse page does not lose any capability.

### Existing directory evaluation

`mosaicfs-server/src/readdir.rs` wraps `mosaicfs_vfs::readdir::evaluate_readdir`
and returns `Vec<ReaddirEntry>`. Each entry has `name`, `file_id`, `inode`,
`size`, `mtime: DateTime<Utc>`, `mime_type`, `source_node_id`,
`source_export_path`, `mount_id`. Mount evaluation is driven by the virtual
directory's `mounts` array plus inherited steps; all matching files for a
directory are returned in a single call (no pagination at this layer).

### Existing open-file logic

`ui/actions.rs open_file_action` takes a `file_id`, resolves the file to a
local path (either the source node's `export_path` directly, or translated via
the longest-prefix `network_mounts` entry), verifies the path exists, and
spawns `open` (macOS) or `xdg-open` (Linux) via `std::process::Command`. Errors
are surfaced through the session flash mechanism; the handler redirects to
`/ui/browse?path=<return_path>`.

### Existing REST API pieces of interest

- `GET /api/search?q=…&label=…&limit=&offset=` — substring/glob match across all indexed files. Not used by this change but documents the prior art.
- `GET /api/files/{file_id}/content` — range-aware file streaming. Not used by this change.

### What is **not** in the codebase today

- No pagination on readdir results.
- No size-formatting filter or case-insensitive in-directory search.
- No "open by virtual path" endpoint — the current page resolves `file_id` into the form at template-render time.
- No chrome-less "app window" layout — the only layout (`layout.html`) has a full nav bar.

---

## 2. Goal

Replace the content of `/ui/browse` with an end-user file browser — launched
as a desktop "app window" (e.g.
`chrome --app=http://localhost:8443/ui/browse`) — that lets users navigate,
search, and open files without any admin affordances on the page. Phase 2
items in `file-browser-requirements.md` (sidebar, keyboard navigation,
recursive search) are out of scope.

---

## 3. Changes

### Change 3.1 — Add a `browse` submodule inside the `ui` router

**Today (post-009).** The only server-rendered UI lives in
`mosaicfs-server/src/ui/`, with the session layer, Tera instance, HTMX asset
route, and `require_auth` middleware all installed by `ui::router()`. The
existing `GET /ui/browse` is handled by `ui::views::browse_page`; its three
sibling URLs (`/ui/browse/list`, `/ui/browse/navigate`, `/ui/browse/open`)
don't exist.

**Proposed.** Add a new submodule `mosaicfs-server/src/ui/browse.rs` and
register four routes inside the existing UI router (`ui/mod.rs`):

- `GET  /ui/browse`          → `browse::page`     — initial page render (replaces `views::browse_page`)
- `GET  /ui/browse/list`     → `browse::list`     — paginated/filtered rows partial
- `GET  /ui/browse/navigate` → `browse::navigate` — toolbar + list partial for dir changes
- `POST /ui/browse/open`     → `browse::open`     — resolve path and spawn OS opener

All four are added to the `protected` branch so they inherit session auth.
`views::browse_page` and the `views::BrowseQuery` struct are deleted; the old
route registration for `GET /ui/browse` is replaced with the new handler.

**Justification.** The UI router already supplies everything the browser
needs: Tera template registry, session-based auth, HTMX and Pico assets, flash
plumbing. A dedicated submodule isolates the handlers (~200 lines of browse-
specific logic) from the unrelated views and actions in the existing files.

### Change 3.2 — Extract shared "open by file_id" helper

**Today (post-009).** `ui::actions::open_file_action` couples four concerns:
(a) HTTP form binding, (b) path resolution from a `file_id` through network
mounts, (c) spawning the OS opener, (d) setting a flash and redirecting.

**Proposed.** Extract (b) and (c) into a helper, either as a function in
`ui/actions.rs` or a new `ui/open.rs` module:

```rust
pub(crate) async fn open_file_by_id(
    state: &AppState,
    file_id: &str,
) -> Result<String /* opened local path */, OpenError>;
```

`OpenError` is an enum that callers can turn into a user-visible string (not
found, no network mount, path does not exist on disk, spawn failed, etc.). The
existing `open_file_action` calls this helper and continues to do its
session/flash/redirect work. `browse::open` calls the same helper after
resolving `path` → `file_id` (§3.3), then returns an HTMX fragment with the
result message.

**Note on caller of `open_file_action`.** Once the new file browser replaces
`/ui/browse`, the `Open` button that currently submits to
`/ui/vfs/open-file` (and redirects back to `/ui/browse`) may become
orphaned — nothing in the new browse page renders that form. Check whether
`open_file_action` is reachable from any other template (it is registered at
`/ui/vfs/open-file` per 009's renamed routes); if not, delete it along with
its form-binding struct. `open_file_by_id` remains because `browse::open`
uses it.

**Justification.** The path-resolution logic is the only non-trivial part of
the open flow, and the new browse handler needs it. Duplicating rather than
extracting would mean the next bug fix lands in two places.

### Change 3.3 — Resolve virtual path to file_id

**Today.** Neither `/api/files/{file_id}` nor `/api/files/by-path` resolves a
*virtual* path (e.g. `/documents/report.pdf`) to a file document. The existing
page resolves `file_id` at render time and puts it in the form.

**Proposed.** Private function in `ui/browse.rs`:

```rust
async fn lookup_entry_by_virtual_path(
    state: &AppState,
    virtual_path: &str,
) -> Option<ReaddirEntry>;
```

Splits the virtual path into `parent_dir + filename`, runs
`readdir::evaluate_readdir` on the parent, returns the entry matching `name`.
This is the single call site that converts the UI's virtual path to the
`file_id` consumed by `open_file_by_id`.

**Justification.** The requirements spec the open endpoint as
`POST /ui/browse/open?path=<full_path>` — path on the wire, not file_id.
Running `evaluate_readdir` on the parent is the same operation the list
endpoint already does, so the cost is bounded. A CouchDB view keyed on
virtual_path is a larger investment; build it only if profiling later shows
the on-click resolution is a bottleneck.

### Change 3.4 — Pagination, server-side search, and sorting

**Today.** `evaluate_readdir` returns the full file list for a directory in
one shot; neither the function nor its callers apply sort, filter, or
limit/offset.

**Proposed.** Inside `browse::list`, after calling `evaluate_readdir`:

1. Apply case-insensitive substring filter on `entry.name` if the request carries a `q` query parameter. Current-directory-only scope matches the Phase 1 requirement.
2. Sort by `sort=name|size|mtime` + `order=asc|desc`. Directories always come first (see §3.5). Name sort is case-insensitive.
3. Slice by `offset`/`limit`. Page size is **50** (per the requirements doc).

No changes are made to `mosaicfs-vfs::readdir` or `mosaicfs-server/src/readdir.rs`
— the logic lives in the browse handler because it is UI-specific.

**Justification.** Requirements call for paginated, filtered, sorted listings.
Pushing those into the shared readdir evaluator would leak UI concerns into a
module also used by FUSE, which already reads the full directory. The
in-handler approach operates on a small in-memory list.

### Change 3.5 — Size formatting filter and directory-first ordering

**Today.** The existing browse template does not show size or date; there is
no size-formatting helper.

**Proposed.** Register a Tera filter `fmt_size` that follows the
requirements-doc rule:

- Exactly 0 bytes → `"0"` (no unit).
- 1…1023 bytes → `"1K"`.
- ≥ 1024 bytes → divide by 1024 and ceil, promoting K → M → G until the integer is ≤ 999.
- Six unit-test fixtures: 0, 1K, 1M, 2M, 999M, 1G.

Directory-first ordering is handled in the handler. The current
`browse_page` at `ui/views.rs:600` already enumerates subdirectories from
CouchDB via `all_docs_by_prefix("dir::", ...)` and filters by `parent_path`;
`browse::page` and `browse::list` reuse that same pattern and prepend the
directories as synthetic rows (`type=dir`, size rendered as `—`).

**Justification.** Size formatting is a small pure function; a Tera filter
keeps the template readable. Directory-first ordering is a stable sort after
prepending.

### Change 3.6 — `browse_app` layout + file-list partial templates

**Today.** The existing `templates/layout.html` renders a horizontal nav with
eight UI links and a logout button. The new browser is launched in Chrome's
`--app=` chrome-less window, so that nav is unwanted visual noise.

**Proposed.** Two new templates, plus deletion of the old browse template:

- `templates/browse_app.html` — minimal layout: `<main>`, CSS/JS `<link>` to the existing `/ui/assets/` URLs, toolbar region, flash slot, content slot. No nav bar. Does not extend `layout.html`; it is its own root document.
- `templates/browse_list.html` — partial rendering rows plus the infinite-scroll sentinel row (`hx-trigger="revealed" hx-get="/ui/browse/list?offset=…"`).
- `templates/browse.html` — the existing admin-oriented file page — **delete**. Its admin affordances remain reachable via `/ui/vfs/*`.

`ui/mod.rs::tera()` is updated: drop `browse.html`, add `browse_app.html` and `browse_list.html`.

**Justification.** Sharing the main layout would force the UI nav into the
app window; forking the layout is cheaper than adding conditionals. The
page + partial pattern matches existing admin panels (`status_panel.html`,
`nodes_panel.html`).

### Change 3.7 — Toolbar behavior: back/forward, location, search

**Today.** None of the toolbar affordances exist on the current `/ui/browse`
page.

**Proposed.** Implemented in `browse_app.html`:

- **Back/Forward** — `<button>`s calling `history.back()` / `history.forward()` via ≤5 lines of inline vanilla JS. Initially disabled; enabled after the first in-app navigation (browser history depth is not reliably introspectable, so once enabled they remain enabled for the rest of the session — clicking past the end of history is a no-op).
- **Location bar** — a form GET-navigating (not HTMX) to `/ui/browse/navigate?path=…`. `onfocus="this.select()"` on the input. Enter submits. On non-existent paths, the handler returns a partial that updates only the flash region; the location input is re-rendered with the last-good path.
- **Search** — an input bound to `hx-get="/ui/browse/list"` with `hx-trigger="keyup changed delay:300ms"`, `hx-target="#file-list"`, `hx-include` pulling the current `path` and `sort`/`order`. Empty `q` returns the unfiltered listing.

No large JS framework. HTMX + two small inline helpers (history and
focus-select) cover the interactivity.

**Justification.** Matches requirements literally. Uses patterns already
established by `hx-get` + `hx-trigger` panels in the existing UI.

### Change 3.8 — Row click behavior: directory vs file

**Today.** The existing browse page uses full-page navigation
(`onclick = location.href = '/ui/browse?path=…'`) for directories and a
separate form-submit button for files.

**Proposed.** In `browse_list.html`:

- **Directory row** → `hx-get="/ui/browse/navigate?path=…"` with `hx-target="#browse-root"`, `hx-swap="outerHTML"`, `hx-push-url="true"`. Updates toolbar (location bar value) and list in a single swap; adds a browser-history entry so Back/Forward works.
- **File row** (the name cell) → `hx-post="/ui/browse/open?path=…"` with `hx-target="#flash"`, `hx-swap="innerHTML"`. Handler returns the flash fragment on error, or empty content on success (silent success per the requirements doc). CSS underlines the filename on hover and sets `cursor:pointer`.

**Justification.** Matches the spec: single left click on file opens, single
left click on dir navigates, back/forward via browser history.

---

## 4. Implementation Phases

Phases are organized topically; intermediate states need not compile or pass
tests.

### Phase 1 — Shared open-file helper

Deliverables:

- `ui::actions::open_file_by_id(&AppState, &str) -> Result<String, OpenError>` (or in new `ui/open.rs`) with the file-lookup, network-mount translation, existence check, and `open`/`xdg-open` spawn.
- `OpenError` enum with variants for every failure path that `open_file_action` currently branches on.
- `open_file_action` (if retained — see §3.2 note) rewritten to call the helper; user-visible flash strings unchanged.
- Unit tests on the helper covering: local file, network-mount translation (longest-prefix match + priority tiebreak), not-found, missing-mount, path-not-accessible.

### Phase 2 — Browse routes, handlers, pagination, sort, search

Deliverables:

- New file `mosaicfs-server/src/ui/browse.rs` exporting four handlers (`page`, `list`, `navigate`, `open`) and `lookup_entry_by_virtual_path`.
- `ui::router()` updated: remove the existing `GET /ui/browse → views::browse_page` route; add the four new browse routes on the `protected` branch.
- Delete `views::browse_page` and `views::BrowseQuery`; leave `views::build_breadcrumbs` only if the new handler uses it (otherwise delete).
- Handler logic: sort (name/size/mtime × asc/desc, directories first, case-insensitive name), case-insensitive substring filter on `name`, `offset`/`limit` slicing with page size 50.
- `browse::open` calls `open_file_by_id` (Phase 1) after resolving the path; returns empty body on success, flash fragment on error.
- Integration tests exercising list pagination, sort, and search against the existing test CouchDB fixtures.

### Phase 3 — Templates, `fmt_size` filter, HTMX wiring

Deliverables:

- `mosaicfs-server/templates/browse_app.html` — minimal standalone layout (no `{% extends "layout.html" %}`) with toolbar, flash region, and file-list container.
- `mosaicfs-server/templates/browse_list.html` — row partial with the infinite-scroll sentinel.
- Delete `mosaicfs-server/templates/browse.html`.
- `fmt_size` Tera filter registered in `ui::tera()`. Unit tests match the six spec examples (0, 1K, 1M, 2M, 999M, 1G).
- Inline CSS for underline-on-hover and the filename-cell cursor, kept in `browse_app.html`.
- Inline JS for `history.back/forward` and location-bar select-on-focus (≤10 lines total; no external scripts).
- `browse_app.html` and `browse_list.html` registered in `ui::tera()`; `browse.html` removed from the list.
- Manual verification: `make run-dev-server`, then `chrome --new-window --app=http://localhost:8443/ui/browse` with `MOSAICFS_INSECURE_HTTP=1`. Exercise navigation, search, sort, open on local and network-mount files.

---

## 5. What Does Not Change

- `mosaicfs-vfs` — no changes to mount evaluation, caches, or `ReaddirEntry`.
- `mosaicfs-common` — no schema changes; no new document types.
- The REST API under `/api/*` — no new endpoints, no changes to existing ones. `/api/search`, `/api/files/*`, `/api/vfs/*` untouched.
- CouchDB schema, document ids, replication, or views.
- The agent crate and its heartbeat/bulk-files endpoints.
- The VFS admin pages at `/ui/vfs/*` — still the home for directory CRUD. This change removes the *duplicate* admin affordances from the browse page, not the primary ones.
- `/ui/vfs/open-file` endpoint — kept if `open_file_action` is still called from anywhere; removed if orphaned (see §3.2 note).
- Deploy manifest, Dockerfile, build pipeline, CI. New templates are `include_str!`-embedded; no new static-asset handling or dependencies.
- Session cookie name (`mosaicfs_session`), scope, or TTL.
- `MOSAICFS_INSECURE_HTTP` bypass behavior.
- macOS-specific development path (`make run-dev-server`) and its CouchDB container.
- The UI nav in `layout.html` — the new browser uses its own `browse_app.html` layout and does not appear in the main nav.

---

## 6. Deferred

- **Phase 2 requirements** (sidebar, keyboard navigation, recursive search). Explicitly deferred in `file-browser-requirements.md`.
- **Linking `/ui/browse` back into the main UI nav.** Once lived-with, decide whether the main UI nav should show a "Browse" link or whether the browser is only reachable via its launcher URL.
- **File preview pane (text/image side panel).** Discussed in `discussion.md` item 4; orthogonal to browse/navigate/open.
- **Auto-refresh on the file list** (`hx-trigger="every Ns"`). Not in requirements; risks confusing per-scroll pagination state.
- **Client-side thumbnails or metadata.** No rendering pipeline exists.
- **Virtual-path index or CouchDB view.** `evaluate_readdir` on the parent is cheap enough for the click-to-open path resolution; build an index only if profiling shows otherwise.
- **Distinct auth scope for `/ui/browse`.** It sits behind the same session cookie as the rest of `/ui`. Multi-user or capability-scoped auth is a separate design.
- **Apply `architecture-doc.patch`.** The sibling patch in this directory pivots the high-level VFS story toward "FUSE-to-CIFS gateway". It is a documentation change and is treated as a separate item — not part of this change's deliverables.
