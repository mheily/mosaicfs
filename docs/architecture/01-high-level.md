<\!-- MosaicFS Architecture · ../architecture.md -->

## PART ONE — High-Level Architecture

MosaicFS is built around three distinct layers that together solve the problem of data sprawl: a lightweight agent that runs on every device, a central control plane that aggregates knowledge from all agents, and a virtual filesystem layer that makes every file accessible to any application. These layers communicate via a replication-based sync protocol, meaning the system works correctly even when devices are offline or intermittently connected.

### Sample Deployment

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────┐
│   Laptop    │  │   Desktop   │  │  NAS Device │  │   Storage   │  │ Browser  │
│             │  │             │  │             │  │             │  │          │
│ macOS/Linux │  │    Linux    │  │    Linux    │  │ S3 / B2 /   │  │  Web UI  │
│ MosaicFS-   │  │ MosaicFS-   │  │ MosaicFS-   │  │ GDrive /    │  │  React / │
│ agent       │  │ agent       │  │ agent       │  │ iCloud /    │  │ PouchDB  │
│ FUSE mount  │  │ FUSE mount  │  │ Control     │  │ OneDrive    │  │          │
│             │  │             │  │ Plane       │  │             │  │          │
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └────┬─────┘
       │                │                │                │               │
       │    CouchDB Replication          │   Storage Backends             │
       │                │                │                │          REST API /
       └────────────────┴───────────────►│◄───────────────┘          PouchDB
                                         │◄──────────────────────────────┘
                                  ┌──────┴──────┐
                                  │Control Plane│
                                  │             │
                                  │ Axum API    │
                                  │ CouchDB     │
                                  └─────────────┘

       ◄────────────────────────────────────────►
          Direct P2P File Transfer (HTTP, same network)
```

---

## Core Components

### The Agent (MosaicFS-agent)

A lightweight background daemon that runs on every physical device — laptops, desktops, NAS units, and any other machine you want to include. The agent has three main responsibilities: crawling the local filesystem to build an index of every file it is configured to watch, monitoring for changes using the operating system's native filesystem notification APIs, and serving file contents to other agents that need them.

Agents are designed to be offline-first. A laptop that goes to sleep and wakes hours later will reconcile with the rest of the system automatically, without any manual intervention. The agent is a single static binary with no runtime dependencies, making deployment to any device straightforward.

### The Control Plane (MosaicFS-server)

The central aggregation point for the entire system. The control plane runs the CouchDB database that holds the authoritative global index of all files across all nodes. It hosts the web user interface and exposes the REST API consumed by agents, the web UI, the CLI, and the desktop file browser.

The control plane is designed to run continuously on an always-on device — typically a NAS or a small cloud instance. It does not handle file bytes for normal operations; it only knows about files, not their contents. This keeps its resource requirements modest: a container with a few hundred megabytes of RAM is sufficient for a home deployment with tens of thousands of files.

### The Virtual Filesystem Layer

A presentation layer that makes every file in the MosaicFS network accessible to any application through standard OS file APIs — open, read, stat — without the application having any awareness of where files physically reside. The layer is read-only in v1.

The virtual filesystem layer is split into two distinct parts:

**Common code** handles everything that is the same regardless of operating system: evaluating virtual directory mount sources to produce directory listings, the tiered file access strategy, the file content cache (both full-file and block modes), download deduplication via shared futures, and cache eviction. This code lives in `mosaicfs-vfs`, a shared library crate used by all OS-specific backends.

**OS-specific backends** implement the interface between the common code and the OS kernel or desktop environment. Each backend registers itself with the OS and translates filesystem calls or URI lookups into calls to the common layer. Four backends are planned:

- **FUSE** (Linux, and macOS fallback) — implemented in v1 using the `fuser` crate. Mounts as a standard filesystem path. The most portable option and the lowest implementation cost.
- **macOS File Provider** — planned for a future version. Apple's modern system extension API for virtual filesystems, replacing kernel extensions. Provides native Finder integration including sync-state badges, on-demand hydration with progress UI, and deep macOS shell integration. Requires a separate app extension written in Swift that communicates with the agent via XPC.
- **Windows Cloud Files API (CFAPI)** — planned for a future version. Microsoft's native sync engine API introduced in Windows 10 1709, the same mechanism used by OneDrive. A minifilter kernel driver (`cldflt.sys`) acts as a proxy between user applications and the sync engine. Creates placeholder files that hydrate on demand. Provides native File Explorer integration including sync-state icons, hydration progress UI, and "Always keep on this device" / "Free up space" context menu options. Implemented as a desktop app component alongside the agent.
- **GIO / KIO** (Linux desktop) — planned for a future version. Rather than a kernel-level filesystem mount, GIO (GNOME) and KIO (KDE) are desktop-level virtual filesystem layers that expose a URI scheme to desktop-aware applications. A MosaicFS GIO backend would register `mosaicfs://` as a scheme, allowing applications like Nautilus, gedit, or any GIO-aware app to open `mosaicfs:///documents/work/report.pdf` directly without a FUSE mount. The KIO equivalent serves the same purpose for KDE applications via Dolphin and KIO-aware apps. This backend is complementary to FUSE rather than a replacement — FUSE provides kernel-level access for all applications, while GIO/KIO provides richer desktop integration (thumbnails, metadata, search provider registration) for desktop-aware applications specifically. Implemented as a GVfs backend (for GIO) and a KIO worker (for KIO), both calling into the MosaicFS REST API or communicating with the local agent via a Unix socket.

The FUSE backend is the only implementation in v1. macOS File Provider, Windows CFAPI, and GIO/KIO are architecturally accommodated — the common VFS code is deliberately decoupled from any OS-specific API — but their implementation is deferred to a future version. On macOS in v1, the FUSE backend is used via macFUSE.

### Storage Backends

A unified abstraction for external storage services — Google Drive, Microsoft OneDrive, Backblaze B2, Amazon S3, iCloud, and local directories. Each storage backend is configured as a `storage_backend` document in CouchDB and can operate in one of three modes: **source** (indexing files from the service into MosaicFS), **target** (replicating MosaicFS files to the service), or **bidirectional**.

When a storage backend has a `hosting_node_id`, only that agent interacts with the service — this is necessary for platform-locked services like iCloud (accessible only on macOS) or local directories. When `hosting_node_id` is omitted, any agent can talk to the service directly — this works for network-accessible services like S3 and B2.

Source-mode backends replace what were previously called "cloud bridges": they poll or watch a cloud service, index file metadata into CouchDB, and serve file bytes on demand. Target-mode backends replace what were previously called "replication targets": they receive file uploads from the agent's replication subsystem. A single backend can serve both roles simultaneously in bidirectional mode.

### The Plugin System

An event-driven extension point that allows external programs to react to file lifecycle events on each agent. Plugins receive structured JSON events when files are added, modified, or deleted, and when a full sync is triggered. They can write back annotations — structured metadata stored in CouchDB and queryable through the step pipeline and search API — or update entirely external systems such as a full-text search engine, a content database, or a remote API.

Two plugin types are supported. **Executable plugins** are invoked by the agent as a child process for each event (or batch of events), receive event data on stdin, and return a JSON result on stdout. They are stateless and require no process management. **Socket plugins** are long-running sidecar processes that the agent connects to over a Unix domain socket. The agent delivers events over the socket and the plugin acknowledges each one; the agent buffers unacknowledged events in a SQLite queue and replays them after a reconnect, making socket plugins resilient to crashes. Socket plugins are suited to workloads that maintain persistent state — a full-text search index, a thumbnail cache, an external database.

Plugins are configured via CouchDB documents and managed through the web UI, taking effect live without restarting the agent. The set of permitted plugins is determined by what executables are present in the agent's plugin directory — a plugin name in a configuration document that has no corresponding binary is a permanent error, not a security bypass. A full sync operation, triggerable per-plugin or globally from the web UI, replays all known files through the plugin pipeline and serves as the recovery mechanism after a crash or for a newly installed plugin catching up on historical files.

---

## Client Applications

MosaicFS is an API-first system. Every capability — indexing, virtual directory management, file access, node status, storage overview, credential management — is implemented as a REST API endpoint on the control plane. The web UI, CLI, and file browser are all clients of that API. Nothing in any client does work that isn't also available to a script or a `curl` command. This has compounding benefits: the CLI serves as a natural test harness for the API during development, the web UI cannot accidentally take shortcuts that bypass the API, and third-party integrations are possible without special support.

### Web Interface

The web interface is the primary day-to-day management surface for most users. It is a React single-page application served by the Axum control plane, using PouchDB to sync directly with CouchDB for live-updating data. It is designed to be fully functional on both desktop browsers and tablet-sized touch screens, making it the recommended interface for devices where neither the agent nor a VFS backend can run — including iPads.

The web interface is organized into eight pages: Dashboard, File Browser, Search, Labels, Nodes, Virtual Filesystem, Storage, and Settings. A persistent left sidebar provides navigation between pages. On narrow viewports the sidebar collapses to a bottom tab bar for touch accessibility. A persistent top bar shows the instance name and a user menu with the current credential name and a logout option. Full page-by-page detail is in the [Web Interface](#web-interface-1) section of Part Two.

### Command-Line Interface (mosaicfs-cli)

The CLI is a thin stateless client that speaks the REST API. It carries no daemon functionality and maintains no local index — it authenticates with an access key and issues API calls. Its primary audience is power users who want to automate tasks, write maintenance scripts, or manage the system from an existing terminal session without opening a browser.

The core command surface:

```
mosaicfs-cli nodes list
mosaicfs-cli nodes status <node-id>

mosaicfs-cli files search <query>            # substring or glob match on filename
mosaicfs-cli files search <query> --json    # machine-readable results
mosaicfs-cli files stat <file-id>
mosaicfs-cli files fetch <file-id> [--output <path>]

mosaicfs-cli vfs ls <virtual-path>           # list directory contents
mosaicfs-cli vfs tree <virtual-path>         # recursive tree view
mosaicfs-cli vfs mkdir <virtual-path>        # create a virtual directory
mosaicfs-cli vfs rmdir <virtual-path>        # delete a directory (--force to cascade)
mosaicfs-cli vfs show <virtual-path>         # show directory document and mounts
mosaicfs-cli vfs edit <virtual-path>         # open directory mounts in $EDITOR as JSON

mosaicfs-cli storage overview
mosaicfs-cli storage history <node-id> [--days 30]

mosaicfs-cli credentials create --name "..."
mosaicfs-cli credentials revoke <access-key-id>
```

The CLI reads its server address and credentials from `~/.config/mosaicfs/cli.toml` or environment variables. Output is human-readable by default; `--json` produces machine-readable output for use in scripts. It is written in Rust as a separate `mosaicfs-cli` binary, sharing data type definitions with the agent codebase via a shared library crate.

### Graphical File Browser (MosaicFS Desktop)

A native desktop application for users who prefer a graphical file management experience. Built with Tauri, which wraps the same React frontend used by the web interface in a lightweight native shell — using the system webview rather than bundling a full browser engine, keeping the binary small and memory footprint low.

The desktop app extends the web interface's file browser with capabilities that require native OS integration:

- Opening files in their associated native application, rather than downloading to a save dialog
- Drag-and-drop from the MosaicFS namespace to the local filesystem
- A persistent window with native OS chrome, dock/taskbar presence, and menu bar integration
- Optional system tray icon with quick-access search

**Write operations** — move, rename, delete — are supported in the desktop app but depend on write-capable API endpoints that are not present in v1. The virtual filesystem layer and REST API are both read-only in the initial release. Write support is planned for a subsequent version; the desktop app's write operations are gated behind this dependency and will be unavailable until the write API is implemented.

**iPad and mobile.** The Tauri desktop app does not run on iOS or iPadOS. For tablet use, the web interface served by the control plane is designed to be installable as a Progressive Web App (PWA) on iPad home screen, with a touch-friendly layout and the full file browser experience. This means no separate mobile app is required.

---

## Data Flow

### Indexing Flow

When an agent starts, it crawls the directories it is configured to watch and writes file metadata documents into its local database. Those documents replicate automatically to the central CouchDB instance on the control plane. The result is a complete, searchable index of every file in the system. Files appear in the virtual namespace when a user navigates to a directory whose mount sources match them — evaluation happens on demand at that point, not at index time.

### File Access Flow

When a user (or application) opens a file through the virtual filesystem mount, the layer looks up the relevant virtual directory's mount sources, determines which node owns the file, and resolves the best way to access it. If the file is local, it is opened directly. If a network mount covers the file's location (such as a CIFS share of the NAS), the file is opened through that mount. If the file is on a remote agent, it is downloaded to a local cache and served from there. The cache is keyed by file UUID and is invalidated when the file's `mtime` or `size` changes — a cached file is never served stale.

### Sync Flow

CouchDB's built-in replication protocol handles all synchronization between agents and the control plane. Each agent maintains a local copy of the metadata database and synchronizes bidirectionally with the control plane in the background. This means VFS directory listings and file metadata are answered entirely from the local database replica — no network round trip is needed for metadata operations, making the filesystem feel fast and responsive even over a slow connection.

---

## The Virtual Filesystem Namespace

One of MosaicFS's most important features is the separation between where files physically live and where they appear in the virtual filesystem. A file stored at a deeply nested, awkwardly named path on one device can appear at a clean, logical path in the virtual tree. Multiple directories from multiple devices can be merged into a single virtual directory. Files that don't match any mount source are invisible to the virtual filesystem layer, keeping the virtual tree free of system noise.

This is controlled by virtual directories — each directory carries a `mounts` array describing what files and subdirectories appear inside it. Mounts support path prefix replacement, flattening entire directory trees, and merging sources from multiple nodes. Filter steps on each mount control which files are included. A file can appear in multiple directories simultaneously — there is no single canonical virtual location. A default directory structure is created on first setup, and the user can refine it over time through the web interface.

---

