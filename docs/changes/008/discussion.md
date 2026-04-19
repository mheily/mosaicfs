# Change 008 Discussion: VFS Management in the Admin UI

_Session date: 2026-04-19_

## The Gap

Change 005 replaced the React/Tauri frontend with a server-rendered admin UI
(Tera + HTMX). The scope of that change deliberately excluded VFS namespace
management — virtual directory configuration, mount sources, and step
pipelines. The reasoning at the time was that file browsing belongs to the
FUSE mount, not the web UI.

What was not accounted for is that configuring the virtual namespace
(creating directories, assigning mount sources, building step pipelines) is a
distinct task from browsing files. The FUSE mount exposes what the namespace
already contains; there is no way to shape that namespace without a management
surface. Change 005 removed the only such surface that existed (the React app)
without providing a replacement.

## Options Considered

### Extend the existing admin UI

Add VFS management pages to the existing Tera/HTMX admin under `/admin/vfs`.
The data model (`VirtualDirectoryDocument`, `MountEntry`, `Step`) and REST API
already exist. The work is primarily template authoring.

The hard part is the step pipeline editor: drag-to-reorder and nested step
cards are awkward in pure HTMX. The agreed approach is to accept a simpler
interaction model for v1 — steps configured via an ordered list with up/down
buttons rather than drag handles. This can be revisited if it proves
genuinely painful in practice.

**Chosen for VFS management.**

### Finder + FUSE mount as the file browser

For the file browsing use case (navigate, search, open files), the FUSE mount
already makes files visible in Finder with Quick Look, Spotlight, and native
open behavior. This is zero new code and covers the core use case.

A custom file browser adds value only for MosaicFS-specific metadata (owning
node, labels, annotations), virtual namespace navigation that does not map
1:1 to the FUSE path, full federated search, or access without a FUSE mount.
These are real use cases but not required for v1.

**Chosen as the MVP filesystem browser.** A native macOS app (SwiftUI) or
web-based file browser is deferred until Finder proves insufficient in
practice.

### Iced desktop app + Leptos web UI (deferred)

A prior session (2026-04-05) explored replacing the admin UI with an Iced
(pure Rust GUI) desktop app and Leptos (Rust/WASM) web UI, with direct
CouchDB access from all peers and the REST API demoted to an internal
implementation detail.

The code-sharing argument is valid — importing `mosaicfs-common` and
`mosaicfs-vfs` directly avoids duplicating step pipeline logic. However,
several concerns led to deferring this direction:

- **Iced API stability.** Major breaking changes between recent releases.
  Maintenance cost on a solo project is real.
- **Non-native aesthetics.** Iced renders via wgpu and does not use native OS
  widgets. For a file browser, the gap from platform conventions is more
  noticeable than for a dashboard.
- **Eliminating the REST API is overreach.** The API has value beyond the
  web UI: agent-to-agent communication, external tooling, future clients.
  Making it an internal detail of Leptos server functions trades a
  well-understood interface for tighter coupling.
- **Two new UI codebases.** Iced + Leptos is still two separate stacks to
  build and maintain, not a simplification.
- **Scope.** The original problem is a missing admin page. The Iced/Leptos
  direction solves a much larger set of problems that are not yet proven
  necessary.

Nothing in the current architecture forecloses this direction later. The core
crates can be imported by a desktop app whenever that work is taken up.

## Decisions

1. **Build the VFS management surface in the existing admin UI.** Virtual
   directory CRUD, mount source forms, and a step pipeline editor are all
   in scope for change 008. The pipeline editor uses up/down reordering
   rather than drag handles for v1.

2. **Finder + FUSE is the MVP file browser.** No new file browsing UI in this
   change. The FUSE mount must be reliable and well-behaved in Finder
   (consistent mount, graceful handling of unavailable remote nodes).

3. **The REST API remains a public contract.** No changes to the API surface
   or its role in the architecture.

4. **A native macOS app is deferred.** SwiftUI is the natural fit if a
   dedicated browser is ever needed, but the decision is deferred until
   Finder + FUSE proves insufficient.

## Scope for Change 008

- `/admin/vfs` — virtual directory tree view (read)
- Create, rename, delete virtual directories
- Mount editor: source node, export path, strategy, source prefix, conflict
  policy
- Step pipeline editor: add/remove/reorder steps, per-step op and fields
- `enforce_steps_on_children` toggle per directory
- Save writes to the existing `virtual_directory` CouchDB documents via the
  existing REST API

Out of scope:
- Live preview panel (`POST /api/vfs/directories/{path}/preview`) — useful
  but not required for v1; can be added incrementally
- Drag-to-reorder for steps — deferred in favour of up/down buttons
- File browsing of any kind
