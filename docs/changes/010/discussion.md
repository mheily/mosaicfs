# Viability: Finder-like Browser UI as FUSE Deferral

## Context

The working Open button (server-side `open` via network-mount path translation) demonstrated
that a browser admin panel can substitute meaningfully for a FUSE mount for browse-and-open
workflows. The question is whether the existing Tera + HTMX stack can be extended into a
genuinely usable file browser, deferring the FUSE implementation.

## Verdict: Viable, with one hard constraint

**The browser UI can match everything the current FUSE implementation does today.**
The FUSE mount is currently read-only (`MountOption::RO`; write ops return `EROFS`).
The browser UI is also read-only for files. Capability parity exists right now.

What the browser adds that FUSE doesn't have yet: search, label display, and the Open button
with network-mount path translation.

What FUSE would add that the browser can't: transparent filesystem access so arbitrary apps
(Preview, VSCode, Terminal) can open files without going through the admin panel. For
workflows where you need to drag a file into an app, FUSE is still necessary. For browse,
search, and open — the browser is sufficient.

---

## What already works (no new code)

| Capability | Status |
|---|---|
| Navigate VFS directory tree | ✓ done — browse page |
| List files per directory (with mount + step evaluation) | ✓ done |
| Open files locally via OS `open` | ✓ done |
| Open remote files via network-mount translation | ✓ done |
| Label display per file | ✓ done |
| MIME type display | ✓ done |
| Create / edit / delete virtual directories | ✓ done |

---

## What HTMX can realistically add

These are medium-effort, high-impact improvements:

### 1. AJAX navigation (no full-page reload on dir clicks)
Replace `onclick="location.href=..."` with `hx-get` + `hx-push-url` + `hx-target="#main-content"`.
Clicking a directory swaps only the file list area; breadcrumb and action buttons update in-place.
This makes the browser feel like a single-page app without any JS build.

### 2. Live search bar
Wire a text input to the existing `GET /api/search` endpoint using
`hx-get="/api/search" hx-trigger="keyup delay:300ms" hx-target="#file-list"`.
The search API already supports substring matching and label filtering. Result: instant
file search across all indexed nodes from within the browse view.

### 3. Auto-refresh file list
`hx-trigger="load, every 30s"` on the file list panel — same pattern already used by
the Status and Nodes panels. The file list stays current as agents index new files.

### 4. File preview panel
For text and image files: a side panel that loads `/api/files/{file_id}/content`
via `hx-get` when a file row is clicked. Images inline; text shows the first N lines.
The content endpoint already supports range requests.

### 5. Sort controls
Server-side query params (`?sort=name`, `?sort=size`, `?sort=mtime`) passed through
`hx-include`. No JS required; the server returns the list pre-sorted.

---

## What is NOT achievable with this approach

| Capability | Why not |
|---|---|
| Rename / delete / move files | Filesystem is read-only by design; files live on source nodes |
| Multi-select with drag-and-drop | Requires JS beyond what HTMX provides |
| Expose files to arbitrary apps | Only FUSE can do this transparently |
| Open files when browser is on a different machine from server | `open` runs on the server; only works when co-located |
| Thumbnail generation | No server-side rendering pipeline exists |

---

## What to build (in priority order)

1. **HTMX navigation + `hx-push-url`** — biggest UX win, eliminates full-page reloads
2. **Live search bar** — wires to existing `/api/search`, works across all nodes
3. **Auto-refresh file list** — keeps list current, already a proven pattern in the admin
4. **File preview panel** — text/image preview using existing content endpoint
5. **Sort controls** — server-side, low effort

Items 1–3 together give a genuinely usable read-only file browser in ~1 session.

---

## Files to modify if proceeding

- `mosaicfs-server/templates/browse.html` — HTMX attributes, search bar, sort controls, preview panel
- `mosaicfs-server/src/admin/views.rs` — `browse_page` to accept sort/search params, extract a renderable file-list partial
- `mosaicfs-server/src/admin/mod.rs` — add `/admin/browse/files` partial endpoint for HTMX swaps
- `mosaicfs-server/templates/layout.html` — no changes needed

Key existing functions to reuse:
- `crate::readdir::evaluate_readdir` — already called in `browse_page`, returns the file list
- `GET /api/search` — existing handler in `mosaicfs-server/src/handlers/search.rs`
- `GET /api/files/{file_id}/content` — existing streaming handler for preview

---

## Verification

- Navigate directories: clicking a row should update the file list without a full page reload
- Search: typing in the search box should filter results within 300ms
- Open: clicking Open on any file should invoke the OS and flash confirmation
- URL: back/forward browser buttons should navigate correctly via `hx-push-url`
