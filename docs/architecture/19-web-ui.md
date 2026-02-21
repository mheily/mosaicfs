<\!-- MosaicFS Architecture · ../architecture.md -->

## Web Interface

The web interface is a React single-page application. PouchDB syncs a filtered subset of the CouchDB database directly into the browser, so most data updates arrive via the live changes feed without explicit polling — file counts increment as agents index, node status badges update when heartbeats arrive, and rule match counts change as the rule engine processes new files. API calls are made via TanStack Query; shadcn/ui provides the component library.

*Note: The UI descriptions below are design guidance for the implementer, not a rigid specification. Layout details, component choices, and interaction patterns may be adjusted during implementation as long as the functional requirements are met.*

### Cross-Cutting Design Patterns

**Node badges.** Wherever the UI references a node — in file metadata, rule sources, search results — the node is rendered as a small colored pill showing its friendly name, never the raw node ID. Hovering the badge shows the node ID, kind, and current status. Clicking navigates to the node detail page.

**Empty states.** Every list view has a purposeful empty state rather than a blank screen. An empty rules list explains what rules are and has a prominent "Create your first rule" button. An empty nodes list links to the agent installation documentation.

**Live data.** PouchDB's change feed drives reactive updates throughout the UI. No manual refresh is needed for most data. Indicators that are live-updated include: node online/offline status, file counts, agent error logs, and storage utilization bars.

**Read-only indicators.** In v1 the filesystem is read-only. Operations that will be available in a future version — move, rename, delete — are rendered as disabled controls with a tooltip explaining they are planned but not yet implemented. This communicates intent without confusion.

**Touch support.** The UI targets iPad as a first-class platform via PWA install. Drag-to-reorder interactions (rule priority, step ordering) use a touch-compatible drag library. The sidebar collapses to a bottom tab bar on narrow viewports.

---

### Dashboard

The entry point after login. Designed to answer two questions within five seconds: "is everything working?" and "what's in my system?"

**Notification bell.** A bell icon in the top navigation bar, present on every page. Shows an unread count badge when there are active or unacknowledged notifications — red for any errors, amber for warnings only, no badge when all is healthy. Clicking opens the notification panel.

**Notification panel.** A slide-in panel from the right, accessible from any page. Notifications grouped by severity — errors first, then warnings, then info. Each notification shows a source badge (node name or "Control Plane"), component label, title, age timestamp, and message. Notifications with `actions` show action buttons inline — e.g. "Re-authorize" for an expired OAuth token. Individual "Acknowledge" button per notification; "Acknowledge all" sweep at the top. A "View history" link navigates to the full notification log. The panel updates in real time via PouchDB live sync — a new error notification appears within seconds of the agent writing it, with no page refresh required.

**Active alert banner.** If any `error`-severity notification is active and unacknowledged, a dismissible banner appears at the top of the dashboard (and only the dashboard — other pages show the bell badge only). The banner summarises the most severe active condition and links to the relevant node or the notification panel. The banner reappears on next page load if the condition persists after dismissal.

**Node health strip.** A horizontal row of cards near the top of the page, one per node. Each card shows the node's friendly name, status badge, online/offline/degraded status badge, and a compact storage utilization bar. Cards are color-coded by status: green for online, amber for degraded, red for offline or unreachable. Clicking any card navigates to that node's detail page.

**Quick-access search bar.** A large, prominent search input centered on the page. Submitting navigates to the Search page with the query pre-filled. Keyboard shortcut (`/` or `Cmd+K`) focuses it from anywhere on the dashboard.

**System totals.** A row of summary stats below the node strip: total indexed files, total virtual paths, total storage capacity across all physical nodes, total cloud storage used. These pull from the PouchDB replica and update live as agents index new files.

**Plugin widgets.** A row of widget cards below the system totals, one per plugin that advertises a `"dashboard_widget"` capability via `query_endpoints`. The control plane polls these nodes on a configurable interval (default 60 seconds) and caches the results. Each widget card shows the plugin's display name, a status badge derived from the widget's `status` field, and a compact set of key-value pairs from the widget's `data` object. Example: a fulltext-search widget might show "Index health: ✓", "Documents indexed: 47,203", "Last sync: 2 minutes ago." Widgets are rendered generically from the key-value data — no plugin-specific UI code. A widget with `status: "warning"` or `status: "error"` is visually highlighted amber or red. Clicking a widget navigates to the plugin's node detail page.

**Recent activity feed.** A scrollable list of the last 50 significant events across all nodes: files added, files deleted, rules evaluated, storage backend syncs completed, errors. Each entry shows a timestamp, the originating node badge, and a short description. Filterable by node via a dropdown. The feed is not a firehose — it shows meaningful events, not every individual file change.

---

### File Browser

A two-panel layout for navigating the virtual filesystem namespace.

**Left panel — directory tree.** A collapsible tree rooted at `/`. Directories expand on click. The tree is lazily loaded — subdirectories fetch their children from `GET /api/vfs?path=...` on first expand rather than loading the entire tree upfront. The selected directory is highlighted. The tree can be collapsed to give the right panel more space.

**Right panel — directory contents.** A sortable table showing files and subdirectories at the selected virtual path. Columns: name (with file type icon), size, last modified, and owning node badge. Sortable by any column. Click a subdirectory to navigate into it (updating both panels). Click a file to open the detail drawer.

**Breadcrumb navigation.** Above the right panel, a clickable breadcrumb showing the current virtual path (e.g. `/ › documents › work › 2025`). Each segment is a link that navigates to that directory.

**Inline search bar.** A filter input above the right panel table. Filters the currently loaded directory listing by filename as you type — client-side, no server round trip. Cleared when navigating to a new directory.

**File detail drawer.** Slides in from the right when a file is selected. Contains:
- Full metadata: virtual path, export path on owning node, owning node badge, size, MIME type, last modified
- **Labels.** The file's effective label set — direct assignments shown as solid-colored pills, labels inherited from a matching rule shown as outlined pills with a tooltip identifying the rule name. An inline text field with autocomplete (from `GET /api/labels`) allows adding new labels directly from this drawer. Clicking the × on a direct label removes it. Inherited labels show "Managed by rule: Work documents" on hover rather than a remove button.
- **Annotations.** One collapsible card per plugin that has annotated this file. Each card shows the plugin display name, `annotated_at` timestamp, and the `data` object rendered as a formatted key-value list (string values inline, nested objects expandable). A "Re-annotate" button deletes the annotation document and enqueues a fresh job. If no plugins have annotated the file, a note explains that annotations appear here after plugins process the file.
- Download button — calls `GET /api/files/{file_id}/content`
- Inline preview for supported types: images render directly, PDFs in an embedded viewer, plain text and Markdown in a scrollable code block. Unsupported types show a generic file icon with the MIME type

**Read-only state.** Move, rename, and delete controls are visible but disabled with a tooltip: "Write operations are coming in a future version." This communicates the roadmap without implying the feature is missing by accident.

---

### Search

A full-page search experience for finding files across the entire virtual namespace.

**Search bar.** Large and central. Results appear after a short debounce (300ms) following the last keystroke — not live character-by-character, but fast enough to feel responsive. The search calls `GET /api/search?q=...` for filename results and `POST /api/query` with `{ capability: "search", query }` for plugin results simultaneously. Both requests fire in parallel; results render as they arrive.

**Label filter chips.** Below the search bar, a row of label chips. Clicking "Add label filter" opens a dropdown populated from `GET /api/labels`. Selected labels appear as dismissible chips and are ANDed with the filename query — the search calls `GET /api/search?q=...&label=X&label=Y`. Each chip can be removed individually. When labels are active and `q` is empty, the search is a pure label filter. Label filters apply only to the filename search section — plugin query sections show their own results unfiltered by label.

**Result sections.** The results area is divided into independent sections, each with its own heading, result count, and result list:

- **Filename matches** — driven by `GET /api/search`. Always present. Shows "No filename matches" if empty. The result interpretation line sits in this section's header: "Searching filenames for `*.pdf`", "Files labelled `important`", etc.
- **Plugin result sections** — one section per result envelope returned by `POST /api/query`. Each section's heading is the plugin's `description` field (e.g. "Full-text search powered by Meilisearch", "Semantic similarity search"). Sections only appear when that plugin returns at least one result. If no nodes advertise `"search"` capability, no plugin sections appear and the experience degrades gracefully to filename-only search. Plugin sections show a spinner while the query is in flight, then render results or a "No results" state.

**Filename match results.** Each result shows:
- File name (with type icon), styled as a link that opens the file detail drawer
- Virtual path in muted text below the name
- Owning node badge
- Label chips for the file's effective labels (direct + inherited), showing up to 3 with a "+N more" indicator
- File size and last modified date on the right

Results load with infinite scroll — new results append as the user scrolls toward the bottom, using `?offset=` pagination on the search endpoint.

**Plugin result items.** Each result in a plugin section is either a file reference or a free-form item. File reference results look identical to filename match results, with the file metadata loaded from PouchDB by `file_id`. An additional `fragments` field, if present, is rendered below the filename as a snippet with matched terms bolded — the canonical fulltext search presentation. Free-form results (no `file_id`) render as a two-column key-value list of the result's fields.

**File detail drawer.** The same component used by the File Browser — clicking any search result opens it, including the full label and annotation UI.

---

### Labels

The label management page. Two tabs: **Assignments** and **Rules**.

**Assignments tab.** A table of all `label_assignment` documents in the system. Columns: node badge, file path, labels (as chips), last updated. Sortable by node and by last updated. A search box filters by path substring. Clicking any row opens the file detail drawer for that file. Useful for reviewing and auditing all manual label assignments across the fleet.

**Rules tab.** A table of all `label_rule` documents. Columns: name, node (or "All nodes" for `node_id: "*"`), path prefix, labels, enabled toggle, created date. An "Add rule" button opens the rule creation form. Clicking a row opens the rule editor.

**Rule editor (drawer).** Fields:
- Name (text field)
- Node selector: "All nodes" or a specific node dropdown
- Path prefix (text field with validation — must end with `/`)
- Labels (tag input with autocomplete from `GET /api/labels`, allowing new labels to be typed freely)
- Enabled toggle

A **live preview panel** below the form shows how many files on the selected node(s) have an `export_path` starting with the given prefix, with up to 10 example filenames. Updates with 500ms debounce as the prefix is edited.

**"Apply to directory" shortcut.** In the Virtual Filesystem and File Browser, right-clicking a directory in the tree offers "Apply labels to this folder and subfolders." This pre-fills the rule creation drawer with the directory's source path (if it has exactly one mount source) or prompts the user to select a node + path if the directory has multiple sources or no sources.

---

### Nodes

A list and detail view for all nodes in the system.

**List view.** A table of all nodes — all agents displayed together. Columns: friendly name, kind, platform, status badge, last heartbeat (relative time, e.g. "2 minutes ago"), indexed file count, and a compact storage utilization bar. A filter control lets the user filter by status. Clicking any row opens the node detail page.

**Physical agent detail page.**

*Header:* Friendly name, node ID, platform badge, overall health badge (healthy / degraded / unhealthy), last heartbeat timestamp. Edit button for the friendly name.

*Status panel:* Per-subsystem health indicators for crawler, watcher, replication, cache, transfer server, and plugins. Each shows a green/amber/red indicator and a short status message. The plugins indicator summarises across all configured plugins — green if all are healthy, amber if any socket plugin is disconnected or any executable plugin has recent failures, red if any plugin has exhausted max_attempts on recent jobs.

*Storage topology:* One card per filesystem in `storage[]`. Each card shows the mount point, filesystem type, device path, a capacity/used bar, and expandable sections for LVM/ZFS volume details and physical disk details when available.

*Utilization trend chart:* A line chart of used bytes over the last 30 days, drawn from `utilization_snapshot` history via `GET /api/storage/{node_id}/history`. One line per filesystem, color-coded. A date range picker allows zooming to 7, 30, or 90 days.

*Watch paths:* A list of the configured paths being indexed on this node.

*Network mounts:* A table of the node's embedded `network_mounts` entries with columns for remote node, remote base export path, local mount path, mount type, and priority. Add, Edit, and Delete controls manage entries via the `/api/nodes/{node_id}/mounts` endpoints.

*Plugins:* A table of plugin configurations for this node. Columns: name, plugin name (the binary), type badge (executable / socket), status indicator, enabled toggle, last activity. An "Add plugin" button opens the plugin editor drawer. Clicking a row expands a detail panel showing recent jobs, failed job count, and a "Sync now" button. A "Sync all plugins" button at the top of the section triggers `POST /api/nodes/{node_id}/sync`. The available plugin names dropdown is populated from `agent_status.available_plugins` — only binaries present in the node's plugin directory are offered.

**Plugin editor drawer.** Fields: display name, plugin name (dropdown of `available_plugins`), plugin type (executable / socket), enabled toggle, subscribed events (multi-select checkboxes for `file.added`, `file.modified`, `file.deleted`, `sync.started`, `sync.completed`), MIME globs (tag input), workers (number field, executable only), timeout (number field, executable only), max attempts (number field, executable only), config (JSON editor with syntax highlighting and validation). Save calls `POST` or `PATCH` as appropriate. Changes take effect on the agent within seconds via the live changes feed — no restart required.

*Recent errors:* A scrollable table of the last 50 errors from the agent's `agent_status` document. Columns: time, subsystem, level (INFO / WARN / ERROR), message. Filterable by level.

**Storage backend detail (node detail subsection).**

The node detail page includes a "Storage Backends" section listing all `storage_backend` documents where `hosting_node_id` matches this node. Each backend shows: name, backend type, mode (source/target/bidirectional), connection status, last sync timestamp. For backends requiring OAuth: authorization status, token expiry, and re-authorize/revoke buttons. For source-mode backends: last poll time, indexed file count, and a "Sync now" button. For target-mode backends: replica count, total replicated size, and a link to the replication status page.

---

### Virtual Filesystem

The primary configuration surface. Presents the virtual directory tree and allows users to create directories, edit their mount sources, and navigate the resulting file listings.

**Tree view (left panel).** A collapsible directory tree showing the full virtual namespace. The root `/` is always expanded on first load. Each row shows a folder icon, directory name, and a badge with the number of direct mount sources. A "New folder" button at the top creates a new directory — the user enters the name and selects a parent directory. Right-clicking a directory opens a context menu: Rename, Edit mounts, New subfolder, Delete.

**Directory contents (right panel).** Clicking a directory in the tree loads its contents — files and subdirectories. Files are listed in a table with columns: name, size, mtime, owning node. Subdirectories show a chevron to expand in place or click to navigate. A breadcrumb trail at the top mirrors the current virtual path.

**Mount editor (drawer).** Opening "Edit mounts" slides in a drawer from the right. The drawer shows:

*Inherit parent steps toggle.* A toggle labeled "Enforce my steps on all subdirectories" (`enforce_steps_on_children`). When on, a visual indicator makes it clear that this directory's steps will propagate down. Any inherited steps from ancestors are shown read-only above the mount list, labeled "Inherited from /parent-path."

*Mount list.* Each mount source is a card showing: source node (dropdown), source path (text field), strategy selector (`prefix_replace` / `flatten`), source prefix field (for `prefix_replace`), conflict policy radio buttons. An "Add mount" button appends a new card. Cards can be reordered by drag handle and removed with a delete button.

*Step pipeline per mount.* Each mount card has an expandable "Filter steps" section. Steps are an ordered list of step cards with the same op controls as before — op selector, op-specific fields, invert toggle, on-match selector. An "Add step" button opens an op type picker. Steps are reorderable. Unknown op types from future versions are shown as read-only "Unknown op" cards preserving their raw JSON.

*Default result toggle.* At the bottom of each mount's steps: "If all steps pass without short-circuiting, the file is: Included / Excluded."

*Live preview panel.* Below all mount cards, a panel showing files that match the current (unsaved) mount configuration. Calls `POST /api/vfs/directories/{path}/preview` with the current draft mounts, updating with a 500ms debounce after any change. Shows up to 20 matching files with their names and owning nodes, plus a total match count. "No matches" state prompts the user to check source paths and step configuration.

*Save and Cancel.* Save calls `PATCH /api/vfs/directories/{path}` with the updated mounts array.

**Delete directory.** Deleting a directory via the context menu shows a confirmation dialog. If the directory has children, the dialog warns and requires the user to confirm cascade deletion. `system: true` directories show a disabled delete option with a tooltip explaining they cannot be removed.

---

### Storage

A system-wide storage overview with current utilization and historical trends.

**Current utilization table.** All nodes in a single table with columns: node name (with kind icon), total capacity, used, available, and a utilization bar. The bar is color-coded: green below 70%, amber 70–90%, red above 90%. Cloud consumption-billed nodes (S3, B2) show used bytes in the "used" column with dashes for capacity and available. iCloud shows an approximate figure with a note icon. The table is sortable by utilization percentage.

**Trend charts.** A node selector (dropdown or tab strip) switches between nodes. The selected node's chart shows used bytes over time, drawn from utilization snapshot history via `GET /api/storage/{node_id}/history`. Physical nodes with multiple filesystems show one line per filesystem, each toggleable via a legend. A date range control offers 7 / 30 / 90 day presets and a custom range picker.

---

### Settings

Five tabs within a single Settings page.

**Credentials tab.** A table of all credentials with columns: name, credential type, created date, last seen, enabled status. A "Create credential" button opens a modal: name field, then on submit displays the generated access key ID and secret key with a copy button and a warning that the secret cannot be retrieved again. An "I've saved this" confirmation button closes the modal. Per-row controls: enable/disable toggle, delete (with confirmation dialog warning that any agent using this credential will immediately lose access).

**Storage Backends tab.** One card per storage backend document showing: backend name, backend type icon, mode badge (source / target / bidirectional), hosting node (if set), connection status (Connected / Disconnected), and token expiry date for OAuth-authenticated backends. An "Authorize" or "Re-authorize" button initiates the OAuth flow for backends requiring OAuth. A "Revoke access" link with confirmation disconnects the backend.

**Plugins tab.** One card per plugin document that has a `settings_schema` declared, grouped by node. Each card shows the plugin's display name, the node it runs on as a node badge, and a form rendered from the `settings_schema`. Field types render as follows: `string` → text input, `number` → number input with optional min/max, `boolean` → toggle, `enum` → select dropdown, `secret` → password input that shows `••••••••` after initial save with an explicit "Change" button to reveal the field for editing. Each field shows its `description` as helper text below the input. Required fields are marked with an asterisk. Default values are shown as placeholder text when the field is empty.

Saving a card writes the new values to `settings` on the plugin document via `PATCH /api/nodes/{node_id}/plugins/{plugin_name}`. Changes replicate to the agent and take effect within seconds — the live changes feed means the plugin receives updated config on its next invocation without any restart. A success toast confirms the save; validation errors (required field missing, value out of range) are shown inline before the request is made.

Plugins without a `settings_schema` do not appear on this tab — they are configured via the raw JSON editor on the node detail page. A note at the bottom of the tab reads "Plugins without a settings schema are configured on the node detail page" with a link.

**General tab.** Configuration fields for: storage backend poll intervals (per backend, in seconds), VFS cache size limit (GB), utilization snapshot retention period (days), and nightly full crawl enable/disable toggle.

**About tab.** Instance information: MosaicFS version, setup date, control plane host. System totals: node count, indexed file count, active rule count, total virtual paths.

**Backup section.** Two download buttons side by side: "Download minimal backup" and "Download full backup". Clicking either calls `GET /api/system/backup?type=minimal` or `?type=full` and streams the JSON file as a download. The button labels include a file size estimate when available. A note below the buttons explains the difference: "Minimal: essential user data only (fast restore, small file). Full: complete database including history (disaster recovery)."

**Restore section.** Conditionally displayed based on `GET /api/system/backup/status`. If the database is empty (`{ empty: true }`), a file upload control appears with the label "Restore from backup". The user selects a JSON backup file, clicks "Restore", and the UI uploads it via `POST /api/system/restore`. A progress indicator shows during the upload and restore. On success, a banner appears prompting the user to restart all agents, with a "Restart all agents" button that calls `POST /api/system/reindex`. On error, the validation errors are shown inline. If the database is not empty, the restore section shows a disabled upload control with a tooltip: "Restore is only permitted into an empty database. To enable restore in a non-empty database, use the DELETE /api/system/data endpoint (requires --developer-mode)."

**System actions.** A "Trigger full reindex" button with a confirmation dialog, calling `POST /api/system/reindex`. A link to the project documentation.

---

