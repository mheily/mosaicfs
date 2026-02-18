# MosaicFS

*A Unified Filesystem of Filesystems*

**Architecture & Design Document — v0.1 Draft**

---

## Table of Contents

**Part One — High-Level Architecture**
- [Problem Statement](#problem-statement)
- [PART ONE — High-Level Architecture](#part-one--high-level-architecture)
  - [Sample Deployment](#sample-deployment)
- [Core Components](#core-components)
  - [The Agent (MosaicFS-agent)](#the-agent-mosaicfs-agent)
  - [The Control Plane (MosaicFS-server)](#the-control-plane-mosaicfs-server)
  - [The Virtual Filesystem Layer](#the-virtual-filesystem-layer)
  - [Cloud Service Bridges](#cloud-service-bridges)
- [Client Applications](#client-applications)
  - [Web Interface](#web-interface)
  - [Command-Line Interface (mosaicfs-cli)](#command-line-interface-mosaicfs-cli)
  - [Graphical File Browser (MosaicFS Desktop)](#graphical-file-browser-mosaicfs-desktop)
- [Data Flow](#data-flow)
  - [Indexing Flow](#indexing-flow)
  - [File Access Flow](#file-access-flow)
  - [Sync Flow](#sync-flow)
- [The Virtual Filesystem Namespace](#the-virtual-filesystem-namespace)
- [Design Decisions](#design-decisions)
- [Security](#security)
  - [Threat Model](#threat-model)
  - [Trust Boundaries](#trust-boundaries)
  - [What the Design Provides](#what-the-design-provides)
  - [Secret Storage at Rest](#secret-storage-at-rest)
  - [Network Exposure](#network-exposure)
  - [Known Gaps and Multi-User Considerations](#known-gaps-and-multi-user-considerations)
- [Federation](#federation)
  - [The Sovereignty Model](#the-sovereignty-model)
  - [Export Modes](#export-modes)
  - [How Federated Peers Map to Existing Concepts](#how-federated-peers-map-to-existing-concepts)
  - [Cross-Instance Authentication](#cross-instance-authentication)
  - [Planned Document Types](#planned-document-types)
  - [v1 Accommodations](#v1-accommodations)

**Part Two — Technical Reference**
- [Technology Stack](#technology-stack)
- [Data Model Overview](#data-model-overview)
  - [Document Types at a Glance](#document-types-at-a-glance)
  - [How the Document Types Relate](#how-the-document-types-relate)
  - [How Each Component Uses the Data Model](#how-each-component-uses-the-data-model)
  - [Replication Topology](#replication-topology)
  - [Soft Deletes and Document Lifecycle](#soft-deletes-and-document-lifecycle)
  - [CouchDB Indexes](#couchdb-indexes)
  - [Replication Flows](#replication-flows)
- [CouchDB Document Schemas](#couchdb-document-schemas)
  - [File Document](#file-document)
  - [Virtual Directory Document](#virtual-directory-document)
  - [Node Document](#node-document)
  - [Credential Document](#credential-document)
  - [Agent Status Document](#agent-status-document)
  - [Utilization Snapshot Document](#utilization-snapshot-document)
  - [Label Assignment Document](#label-assignment-document)
  - [Label Rule Document](#label-rule-document)
  - [Plugin Document](#plugin-document)
  - [Annotation Document](#annotation-document)
  - [Notification Document](#notification-document)
- [FUSE Inode Space](#fuse-inode-space)
- [FUSE Tiered Access Strategy](#fuse-tiered-access-strategy)
- [Authentication](#authentication)
  - [Credential Format](#credential-format)
  - [Agent-to-Server: HMAC Request Signing](#agent-to-server-hmac-request-signing)
  - [Web UI: JWT Sessions](#web-ui-jwt-sessions)
  - [Agent-to-Agent: Credential Presentation](#agent-to-agent-credential-presentation)
- [REST API Reference](#rest-api-reference)
  - [Auth](#auth)
  - [Nodes](#nodes)
  - [Node Network Mounts](#node-network-mounts)
  - [Files](#files)
  - [Virtual Filesystem](#virtual-filesystem)
  - [Search](#search)
  - [Labels](#labels)
  - [Plugins](#plugins)
  - [Annotations](#annotations)
  - [Query](#query)
  - [Notifications](#notifications)
  - [Credentials](#credentials)
  - [Storage](#storage)
  - [System](#system)
  - [Agent Internal](#agent-internal)
- [Backup and Restore](#backup-and-restore)
  - [Minimal Backup](#minimal-backup)
  - [Full Backup](#full-backup)
  - [Backup Format](#backup-format)
  - [Restore Process](#restore-process)
  - [What Is Not Backed Up](#what-is-not-backed-up)
  - [Triggering Backups](#triggering-backups)
- [Agent Crawl and Watch Strategy](#agent-crawl-and-watch-strategy)
  - [Initial and Periodic Crawl](#initial-and-periodic-crawl)
  - [Incremental Watching](#incremental-watching)
  - [inotify Watch Limit](#inotify-watch-limit)
  - [Reconciliation After Reconnect](#reconciliation-after-reconnect)
  - [Agent Main Loop](#agent-main-loop)
- [Rule Evaluation Engine](#rule-evaluation-engine)
  - [Evaluation Model](#evaluation-model)
  - [Step Pipeline](#step-pipeline)
  - [Mapping Strategies](#mapping-strategies)
  - [Conflict Resolution](#conflict-resolution)
  - [Readdir Evaluation](#readdir-evaluation)
- [Plugin System](#plugin-system)
  - [Plugin Runner Architecture](#plugin-runner-architecture)
  - [Plugin Full Sync](#plugin-full-sync)
  - [Available Plugins Discovery](#available-plugins-discovery)
  - [Capability Advertisement](#capability-advertisement)
  - [Plugin Query Routing](#plugin-query-routing)
  - [Bridge Nodes](#bridge-nodes)
  - [Plugin Security Model](#plugin-security-model)
  - [Future Directions](#future-directions)
- [Search](#search-1)
  - [v1: Filename Search](#v1-filename-search)
  - [Future: Richer Search](#future-richer-search)
- [VFS File Cache](#vfs-file-cache)
  - [On-Disk Structure](#on-disk-structure)
  - [SQLite Index Schema](#sqlite-index-schema)
  - [Block Map](#block-map)
  - [Full-File Mode Request Flow](#full-file-mode-request-flow)
  - [Block Mode Request Flow](#block-mode-request-flow)
  - [Transfer Integrity](#transfer-integrity)
  - [Eviction](#eviction)
  - [Invalidation](#invalidation)
  - [Download Deduplication](#download-deduplication)
- [Deployment](#deployment)
  - [Control Plane](#control-plane)
  - [Agents](#agents)
  - [State Directory](#state-directory)
- [Observability](#observability)
  - [Logging](#logging)
  - [Health Checks](#health-checks)
  - [Error Classification](#error-classification)
- [Cloud Service Bridges](#cloud-service-bridges-1)
  - [Bridge Interface](#bridge-interface)
  - [Per-Service Notes](#per-service-notes)
  - [Polling Strategy](#polling-strategy)
- [Web Interface](#web-interface-1)
  - [Cross-Cutting Design Patterns](#cross-cutting-design-patterns)
  - [Dashboard](#dashboard)
  - [File Browser](#file-browser)
  - [Search](#search-2)
  - [Nodes](#nodes-1)
  - [Virtual Filesystem](#virtual-filesystem-1)
  - [Storage](#storage-1)
  - [Settings](#settings)

---

## Problem Statement

Modern power users accumulate data across laptops, desktops, NAS devices, virtual machines, and multiple cloud services. No single tool provides a unified view of all that data or a consistent way to access it. MosaicFS solves this with a peer-to-peer mesh of agents that index every file in every location, a central control plane that aggregates that knowledge, and a virtual filesystem layer that presents everything as a single coherent tree — accessible from any device, to any application that can open a file.

---

## PART ONE — High-Level Architecture

MosaicFS is built around three distinct layers that together solve the problem of data sprawl: a lightweight agent that runs on every device, a central control plane that aggregates knowledge from all agents, and a virtual filesystem layer that makes every file accessible to any application. These layers communicate via a replication-based sync protocol, meaning the system works correctly even when devices are offline or intermittently connected.

### Sample Deployment

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────┐
│   Laptop    │  │   Desktop   │  │  NAS Device │  │Cloud Bridges│  │ Browser  │
│             │  │             │  │             │  │             │  │          │
│ macOS/Linux │  │    Linux    │  │    Linux    │  │GDrive / S3  │  │  Web UI  │
│ MosaicFS-   │  │ MosaicFS-   │  │ MosaicFS-   │  │  B2/iCloud  │  │  React / │
│ agent       │  │ agent       │  │ agent       │  │  OneDrive   │  │ PouchDB  │
│ FUSE mount  │  │ FUSE mount  │  │ Control     │  │ Control     │  │          │
│             │  │             │  │ Plane       │  │ Plane       │  │          │
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └────┬─────┘
       │                │                │                │               │
       │    CouchDB Replication          │    CouchDB Replication         │
       │                │                │                │          REST API /
       └────────────────┴───────────────►│◄───────────────┘          PouchDB
                                         │◄──────────────────────────────┘
                                  ┌──────┴──────┐
                                  │Control Plane│
                                  │             │
                                  │ Axum API    │
                                  │ CouchDB     │
                                  │ Cloud Bridge│
                                  │ Runners     │
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

The central aggregation point for the entire system. The control plane runs the CouchDB database that holds the authoritative global index of all files across all nodes. It hosts the web user interface, runs the cloud service bridges, and exposes the REST API consumed by agents, the web UI, the CLI, and the desktop file browser.

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

### Cloud Service Bridges

Adapters that connect cloud storage services into the MosaicFS namespace. Each bridge runs as part of the control plane and presents its cloud service as a virtual node in the system. Supported services include Google Drive, Microsoft OneDrive, Backblaze B2, Amazon S3, and iCloud (via the local sync directory on macOS).

Bridges poll their respective cloud APIs on a schedule and publish file metadata into the central database. When the virtual filesystem layer needs to access a cloud file, it either opens the locally-mounted cloud sync directory directly (if available on that device) or requests the file through the control plane bridge.

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

When a user (or application) opens a file through the virtual filesystem mount, the layer looks up the relevant virtual directory's mount sources, determines which node owns the file, and resolves the best way to access it. If the file is local, it is opened directly. If a network mount covers the file's location (such as a CIFS share of the NAS), the file is opened through that mount. If the file is on a remote agent, it is downloaded to a local cache and served from there. The cache is keyed by node ID and export path and is invalidated when the file's `mtime` or `size` changes — a cached file is never served stale.

### Sync Flow

CouchDB's built-in replication protocol handles all synchronization between agents and the control plane. Each agent maintains a local copy of the metadata database and synchronizes bidirectionally with the control plane in the background. This means VFS directory listings and file metadata are answered entirely from the local database replica — no network round trip is needed for metadata operations, making the filesystem feel fast and responsive even over a slow connection.

---

## The Virtual Filesystem Namespace

One of MosaicFS's most important features is the separation between where files physically live and where they appear in the virtual filesystem. A file stored at a deeply nested, awkwardly named path on one device can appear at a clean, logical path in the virtual tree. Multiple directories from multiple devices can be merged into a single virtual directory. Files that don't match any mount source are invisible to the virtual filesystem layer, keeping the virtual tree free of system noise.

This is controlled by virtual directories — each directory carries a `mounts` array describing what files and subdirectories appear inside it. Mounts support path prefix replacement, flattening entire directory trees, and merging sources from multiple nodes. Filter steps on each mount control which files are included. A file can appear in multiple directories simultaneously — there is no single canonical virtual location. A default directory structure is created on first setup, and the user can refine it over time through the web interface.

---

## Design Decisions

| Decision | Rationale |
|---|---|
| **Rust for agent & VFS layer** | The agent and virtual filesystem backends are filesystem daemons where memory safety bugs can cause data corruption. Rust eliminates this class of bugs at compile time. The primary developer has a C/C++ systems background and is already comfortable with Rust, making it the natural choice over Go (which would require learning a new language while building a complex system). |
| **React + Vite for web UI** | The developer is an experienced systems programmer, not a frontend specialist, and will rely on AI tooling to generate and maintain the UI. React has the largest corpus of training data among AI models, producing more reliable code generation than newer frameworks like Svelte. Vite provides fast hot-reload during UI iteration. |
| **CouchDB + PouchDB for sync** | The replication problem between agents and the control plane is a solved problem in CouchDB. Offline-first operation, conflict detection, and incremental sync come for free. The live changes feed enables real-time updates in both the VFS layer and the browser UI without custom WebSocket infrastructure. |
| **Path-based cache with per-transfer integrity** | The VFS file cache keys entries by `{node_id}::{export_path}`, invalidated when `mtime` or `size` changes in the file document. This eliminates the need to compute a content hash for every file. Transfer integrity is provided by an HTTP `Digest` trailer (RFC 9530, `sha-256`) on full-file responses — the serving agent computes the hash as it streams and appends it as a trailer; the receiving agent verifies after the stream completes. Range responses (HTTP 206) do not carry a `Digest` trailer and rely on TLS for in-transit integrity. This cleanly separates file identity (path + mtime + size) from transfer integrity (per-response digest), avoids storing a property on the file document that is expensive to compute and only used by the cache, and handles the streaming-hash problem by using trailers rather than headers. |
| **`mosaicfs_browser` read-only CouchDB user for browser sync** | The browser is a larger attack surface than an agent — it can be compromised by XSS, a malicious extension, or a hijacked session in ways that a daemon process cannot. Rather than proxying the full CouchDB replication protocol to an authenticated browser session (which would allow push access to the database), the control plane creates a restricted CouchDB role at setup time with read-only access to a scoped document set. The Axum login endpoint issues a short-lived session token for this role alongside the JWT. Push attempts are rejected by CouchDB's own permission model, not by filter logic that could be misconfigured. A compromised browser session can read indexed file metadata but cannot modify rules, disable credentials, or corrupt the database. |
| **Sorted interval list for block map** | The block cache tracks which regions of a large file are present using a sorted list of non-overlapping `[start, end)` block intervals rather than a raw bitmap. For the home media use case — a user watching a video with a few seeks — this produces 3–15 intervals regardless of file size, serializes to ~40–120 bytes in SQLite, and makes the "find missing ranges to fetch" operation natural. A raw bitmap is O(n) in file size; the interval list is O(k log k) in the number of intervals. A roaring bitmap is a suitable future upgrade for workloads with highly fragmented random access (e.g. large database files), but is unnecessary for video and audio streaming. |
| **Directories own their mount rules** | Rather than maintaining a separate collection of rule documents evaluated against every file, each virtual directory carries a `mounts` array describing what gets mounted inside it. This makes the directory the natural owner of its configuration — editing a directory's mounts is done by navigating to it and changing its properties, not by finding and modifying an abstract rule elsewhere. It also eliminates the global priority ordering problem: mount priorities are local to one directory's `mounts` array, not a system-wide ranking. A file can appear in multiple directories simultaneously, which is an explicit feature rather than a conflict to resolve. |
| **On-demand rule evaluation, no background worker** | Rules are evaluated when a directory is accessed (`readdir`), not pre-computed and stored on file documents. This means rule changes take effect on the next directory listing rather than after a background recomputation cycle. The VFS layer caches `readdir` results for a short TTL to avoid re-evaluating mounts on every `lookup`, invalidated by the PouchDB live changes feed. The tradeoff is that `readdir` is slightly more expensive than reading pre-indexed values, but at home-deployment scale this is imperceptible. |
| **`virtual_path` not stored on file documents** | A file's location in the virtual tree is a derived property, not an intrinsic one. Not storing it acknowledges this honestly and avoids a class of staleness bugs where stored virtual paths outlive the rules that generated them. It also enables the multiple-appearances feature naturally — a file that belongs in three directories has no single canonical virtual path to store. Search operates on intrinsic file properties in v1; virtual-path-aware search can be added later if needed. |
| **Labels stored separately from file documents** | Labels are user-defined metadata that must survive the agent crawler rewriting file documents on every re-crawl. Storing labels on the file document would require the crawler to merge existing labels on every write — a fragile, stateful operation. Separate `label_assignment` documents keyed by `node_id + export_path` are never touched by the crawler, making the invariant simple and the crawler stateless. Label rules are separate documents rather than embedded in virtual directories because labels are a cross-cutting concern — a label rule should apply to a file everywhere it appears, not only when accessed through a particular directory. |
| **Plugin filesystem directory as the security boundary** | Plugin configurations are stored in CouchDB and replicated to agents — any user with write access to the database can create or modify a plugin document. Allowing the `plugin_name` field to be an arbitrary command path would make CouchDB write access equivalent to remote code execution on every agent. Restricting execution to a fixed, admin-controlled directory (`/usr/lib/mosaicfs/plugins/` on Linux) means the database controls which plugins are *configured*, while the filesystem controls which plugins are *permitted to run*. Installing a plugin binary requires local admin access to the machine; this is the appropriate trust boundary. Path traversal in `plugin_name` is rejected as a permanent error. |
| **Plugin config in CouchDB, not agent.toml** | Storing plugin configuration in the database rather than in per-machine config files enables web UI management, live reloading without agent restarts, and consistent configuration across a fleet without SSH access to individual machines. The agent watches the changes feed for its own node's plugin documents and reloads configuration within seconds of a change. The only local configuration required is the presence of the plugin binary itself in the plugin directory. |
| **Separate SQLite job queue for plugins** | Plugin jobs need durability across agent restarts — a queue in memory would lose pending jobs on crash. The VFS cache already uses a SQLite sidecar for the block index; a separate `plugin_jobs.db` uses the same infrastructure without mixing concerns. Keeping the plugin queue in a separate file means the cache and the plugin runner can be developed, backed up, and debugged independently. |
| **Full sync as the crash recovery mechanism** | Rather than implementing complex exactly-once delivery guarantees for the event stream, the system accepts that crashes can create gaps and provides a user-triggered full sync as the recovery path. The full sync is idempotent — it compares `annotation.annotated_at` against `file.mtime` and skips files that are already current. This is also the natural onboarding mechanism for newly installed plugins, which need to process all historical files. The tradeoff is that recovery requires a user action rather than being fully automatic, which is acceptable for a home deployment. |
| **Capability-based query routing, plugin identity in the response** | The browser does not know what plugins are installed or which nodes run them. It sends a query with a capability name (`"search"`) to `POST /api/query` and receives an array of labelled result envelopes. The control plane fans the query to all nodes advertising that capability in their `node.capabilities` array; each response envelope carries the plugin's `description` so the UI can label each result section meaningfully. This means adding a new search backend — semantic search, OCR search — requires only deploying a new plugin binary and declaring a `query_endpoints` entry in its plugin document. No control plane code changes, no UI code changes. The UI degrades gracefully when no nodes advertise a capability, rendering that result section simply absent. |
| **Plugin-agent as a standard node in Docker Compose** | Query-serving plugins (fulltext search, semantic search) need to run on a machine with access to the Compose internal network so they can reach services like Meilisearch by hostname. Rather than introducing a new node kind or a special control plane plugin host concept, a standard MosaicFS agent running in the Compose stack satisfies this requirement without any changes to the data model or agent binary. It registers as a regular `node_kind: "physical"` node with no watch paths and no VFS mount. The only operationally meaningful difference is that it has no `storage` array and `vfs_capable` is false — properties the UI already handles for cloud bridge nodes. The indexing plugin (one per physical agent, writes to Meilisearch on file events) and the query plugin (one on the plugin-agent node, reads from Meilisearch on user queries) are two separate binaries sharing the same external service, cleanly separating data pipeline concerns from query concerns. |
| **Plugin settings schema declared in the plugin document** | Plugins declare their user-configurable settings as a JSON Schema-subset object in `settings_schema`. Values entered by the user are stored in a separate `settings` field on the same document. The agent merges `settings` into `config` at invocation time — the plugin binary receives one flat `config` object regardless of which keys came from which source. This separation means the raw `config` field remains available for advanced users and scripted deployments while `settings_schema` enables the UI to render a proper form without any plugin-specific UI code. Supported field types — `string`, `number`, `boolean`, `enum`, `secret` — cover the common configuration surface area for plugins. `secret` fields are stored as plaintext in CouchDB (the database is not externally accessible) but displayed only as `••••••••` in the UI after initial entry, preventing casual observation. |
| **`notification` as a first-class document type with stable deduplication keys** | Notifications from agents, bridges, the control plane, and plugins all share a single document type rather than being surfaced only through `agent_status` error arrays. This gives the browser a single PouchDB query to watch for all system events, enables real-time delivery via the live changes feed without polling, and allows the UI to show a unified notification bell across all pages. The deterministic `_id` scheme — `notification::{source_id}::{condition_key}` — means a recurring condition is an upsert rather than an accumulation of duplicates. Separating `first_seen` from `last_seen` and tracking `occurrence_count` gives the UI enough information to say "this condition has occurred 47 times since Feb 14" without storing a full event log per notification. Auto-resolving notifications (written by the source when the condition clears) keeps the active notification set clean without requiring manual user intervention for transient issues. |
| **Bridge nodes unify data-source adapters with the existing agent model** | Rather than introducing a separate node kind or a dedicated bridge process, external data sources (email, calendar, cloud APIs) are modeled as standard agents with no watch paths and a `provides_filesystem` plugin acting as their filesystem implementation. This means bridge nodes participate in the same document model, replication flows, health monitoring, notification system, and plugin infrastructure as physical nodes — no new code paths for the control plane or browser. The `role: "bridge"` field is purely a UI hint; it has no effect on agent behavior. Bridge storage is permanent data storage, not a cache — the user controls retention via plugin settings (`auto_delete_days: 0` to keep everything), and the agent monitors both disk and inode utilization and writes notifications when either approaches capacity. |
| **Plugin materialize via VFS cache staging path** | Filesystem-providing plugins that use aggregate storage (Option B) materialize files on demand by writing to `cache/tmp/` — the same staging directory used for Tier 4 remote downloads. The agent moves the staged file into the VFS cache using the standard atomic rename and path-keyed cache entry, then serves from cache. This means all subsequent accesses hit the cache without plugin involvement, range requests and LRU eviction work identically to remote files, and the plugin implementation is trivial — write bytes to a path, return the size. No new streaming protocol, no in-process byte handling, no Digest trailer to compute. The cache `source` column distinguishes plugin-materialized entries from remote downloads for diagnostic purposes only. |
| **DELETE /api/system/data gated by developer mode** | Database wipes are dangerous in production and should require destroying the Docker Compose stack. But during development and testing, being able to quickly cycle between backup/restore states via an API call is valuable. The `--developer-mode` flag on the control plane binary gates access to `DELETE /api/system/data` — enabled for development workflows, disabled by default for production safety. The web UI never exposes this operation; it's API-only and requires a confirmation token in the request body. The intended use is scripted integration tests and local development, not production operation. |
| **Plugin health checks via pull, not push** | Socket plugins are polled for health status on a configurable interval rather than being given a mechanism to push notifications at arbitrary times. The agent's existing heartbeat loop provides the natural cadence; the health check message is a small addition to the socket protocol. This is simpler than managing unsolicited inbound messages on the socket in v1, tolerates the latency of a polling interval (acceptable for operational health reporting), and the socket remains available for a future push extension — an unsolicited `{ "type": "notification" }` message from the plugin would require only a small addition to the inbound message handler without changing the document model or notification lifecycle. |
| **Parent controls step inheritance** | The `enforce_steps_on_children` flag lives on the parent directory, not on child mounts. A parent that sets this flag prepends its steps to every mount evaluation in all descendant directories. Children cannot opt out. This matches how filesystem permissions feel to users — a parent sets policy for its subtree. A child can always add further restrictions on top; it cannot bypass what the parent has decided. |
| **64-bit only** | Inode numbers are random 64-bit integers assigned at document creation time. Supporting 32-bit platforms would reduce the inode space to 32 bits, making collisions a real concern at scale. A compile-time error on 32-bit platforms is cleaner than a silent correctness problem. |
| **Inodes stored in CouchDB** | Storing inode numbers in the database rather than a local sidecar ensures that all nodes running any VFS backend see the same inode for the same file. This makes tools that cache by inode (editors, build systems, backup tools) behave correctly across machines. The inode concept is used directly by FUSE and maps to equivalent identity concepts in macOS File Provider and Windows CFAPI. |
| **Control plane owns bridges** | Cloud service bridges run on the control plane rather than on individual user devices. This ensures bridge availability is tied to the always-on control plane rather than to whichever laptop happens to be awake. It also centralizes credential management and simplifies the security model. |
| **Read-only virtual filesystem in v1** | Write support introduces a large class of problems: conflict resolution when the same file is modified on two nodes, ordering of writes through the cache, propagation delays. Deferring this to a later version lets the initial implementation focus on correctness of the read path. All three planned OS backends (FUSE, macOS File Provider, Windows CFAPI) are read-only in v1. |
| **HMAC request signing** | Agent-to-server authentication uses HMAC-SHA256 request signing with a timestamp to prevent replay attacks. This is well-understood, stateless, and requires no session management on the server. The AWS Signature V4 convention was chosen as the naming model because it is familiar to technical users. |
| **Tiered file access** | The common VFS layer tries access methods in order of increasing cost: local file → network mount (CIFS/NFS) → locally-mounted cloud sync → remote HTTP fetch. This logic is shared across all OS-specific backends and ensures that files are served via the cheapest available path without the user having to configure anything beyond declaring what network mounts exist. |
| **Preshared keys for v1** | A single-user home deployment does not need per-resource permissions or multi-user access control. Preshared key pairs (styled after AWS access keys) are simple to implement, well-understood, and sufficient for the intended deployment scenario. The credential document schema is designed to accommodate scoped permissions in a future version. |
| **API-first architecture** | All functionality is implemented as REST API endpoints before any client is built. This ensures that the CLI, web UI, and file browser are true equals — no client has privileged access to functionality unavailable to the others. It also means automation and third-party integrations are possible without special support. |
| **Web UI as primary interface** | The web interface is the recommended management surface for most users. It is served by the control plane, requires no installation, and works on any device with a browser — including tablets where a VFS backend and the agent cannot run. The PWA capability makes it installable on iPad for a near-native experience. |
| **Tauri for desktop file browser** | Tauri wraps the same React frontend used by the web interface, avoiding a separate UI codebase. It uses the system webview rather than bundling Chromium, making the binary significantly smaller and lighter than an Electron equivalent. Native OS integration (file associations, drag-and-drop, system tray) requires a native shell that a web app alone cannot provide. |
| **CLI as API client only** | The CLI carries no daemon functionality and maintains no local state. It is a thin wrapper around the REST API, which means it requires no installation beyond a single binary and no special permissions. It serves as a natural test harness for the API during development and as a scriptable interface for automation. |
| **File browser write operations deferred** | Move, rename, and delete operations in the desktop file browser require a write-capable REST API that does not exist in v1. The virtual filesystem layer is also read-only in v1. Attempting to implement writes in only one client while others remain read-only would create an inconsistent user experience. Write support is planned as a cohesive v2 feature covering the API, VFS layer, and all clients simultaneously. |
| **Filename search in v1, richer search deferred** | Filename and virtual path search is cheap — it operates entirely on data already in CouchDB with no additional infrastructure. Full-text content search requires a separate indexing pipeline, extraction of file contents from remote nodes, and a dedicated search engine (Meilisearch or Tantivy). The complexity and resource cost of content search is out of scope for v1. Metadata filtering (type, size, date, node) and content search are planned for future versions. |
| **Federation over multi-tenancy** | Multi-user support within a single instance requires rearchitecting replication, the rule engine, and every API endpoint to enforce per-user access control. Federation sidesteps this by keeping each instance single-user and making cross-user sharing an explicit, opt-in boundary between sovereign instances. The security model stays simple within each instance; complexity lives at the federation layer, which is optional and additive. |
| **Unified `export_path` for all source types** | Rule sources originally used `real_path`, implying a filesystem path on a physical machine — a concept that doesn't apply to cloud bridges or federated peers. Renaming to `export_path` makes the field honest for all node types: filesystem path for physical agents, cloud service path for bridges, and virtual path for federated peers. This gives the rule engine a single code path for resolving sources regardless of node type, and makes merge rules that span local nodes, cloud bridges, and federated peers expressible with a uniform schema. |

---

## Security

### Threat Model

MosaicFS is designed for a single owner operating a private home network. The relevant threats are an attacker on the same local network attempting to intercept or access file data, and accidental exposure of the control plane to the internet. It is not designed to defend against a malicious device owner — an attacker with physical access to a machine running an agent is outside the threat model. It is also not a multi-user system in v1; there is no concept of one user's files being hidden from another.

### Trust Boundaries

There are four trust boundaries in the system:

**The control plane** is the most trusted component. It holds the authoritative database, manages credentials, and runs cloud bridge tokens. It should run on a machine you physically control and trust — a home NAS or a private cloud instance. Nothing in the system should be more exposed than the control plane.

**Physical agents** are trusted once they have presented a valid credential. An agent that has been issued a key can read any file in the system, push documents into the global index, and authenticate with other agents. A compromised agent is a meaningful threat — it can exfiltrate any file it can reach. Credential revocation via the control plane immediately cuts off a compromised agent from the control plane and from other agents (whose local credential replicas will update within minutes).

**Cloud bridges** run inside the control plane process and inherit its trust level. OAuth tokens for cloud services are stored in encrypted files on the control plane host and are not replicated to agents.

**Clients** — the web UI, CLI, and desktop app — are trusted to the extent their credential allows. The CLI and desktop app use HMAC-signed access key credentials, the same mechanism as agents. The web UI uses a restricted read-only CouchDB session for live data sync (see below) plus JWT-authenticated REST API calls for mutations. A leaked CLI credential grants the same broad access as an agent credential; a hijacked browser session is limited to read access on the database.

### What the Design Provides

- **TLS on all external connections.** The control plane generates a self-signed CA and server certificate at setup time. Agents and clients verify the server certificate against this CA. All traffic between clients and the control plane is encrypted in transit.
- **HMAC-signed requests prevent replay attacks.** Agent requests include a timestamp; the control plane rejects requests with a timestamp older than five minutes. An intercepted request cannot be replayed after that window.
- **Credentials stored as Argon2id hashes.** Secret keys are hashed with Argon2id on first presentation and never stored in recoverable form. A database dump does not expose usable credentials.
- **CouchDB bound to localhost only.** CouchDB is not directly reachable from the network. The Axum server is the only externally-accessible process. Agent-to-CouchDB replication runs through an Axum-proxied endpoint authenticated with HMAC credentials. Browser clients do not use this proxy — instead, the Axum login endpoint issues PouchDB a short-lived session token for a restricted CouchDB user (`mosaicfs_browser`) that has read-only access to a scoped subset of the database. Push attempts from the browser are rejected by CouchDB's own permission model, not by filter logic. This means a hijacked browser session cannot modify rules, disable credentials, or corrupt the index — the worst it can do is read documents the browser filter allows.
- **Agent-to-agent transfers are authenticated.** Transfer requests between agents use the same HMAC signing as agent-to-control-plane requests, validated against the local credential replica. The control plane does not need to be reachable for P2P transfers to be authenticated.
- **Secret keys are never logged or passed as CLI arguments.** The agent init command reads the secret key from stdin with echo disabled. The `MOSAICFS_SECRET_KEY` environment variable is available for scripted deployments, but the key is never accepted as a positional argument that would appear in shell history or process listings.

### Secret Storage at Rest

| Location | What is stored | How |
|---|---|---|
| Control plane host | CouchDB admin credential | Docker Compose environment file, readable only by the compose service user |
| Control plane host | Cloud bridge OAuth tokens | Encrypted files in `bridges/`, key derived from a host secret at startup |
| Agent host | Agent access key ID and secret | `agent.toml`, file permissions `0600`, owned by the agent service user |
| CLI user machine | CLI access key ID and secret | `~/.config/mosaicfs/cli.toml`, file permissions `0600` |
| Browser | Web UI session JWT (for REST API calls) | In-memory only — never written to `localStorage` or cookies |
| Browser | PouchDB session token for `mosaicfs_browser` CouchDB user | In-memory only, short-lived, read-only scope |

### Network Exposure

The control plane exposes one port externally: the Axum HTTPS API server (default 8443). CouchDB listens on localhost only and is not directly reachable from outside the host. Agent-to-CouchDB replication runs through an Axum-proxied endpoint, authenticated with HMAC credentials before the connection is passed through. Browser clients connect to CouchDB directly via PouchDB using a short-lived session token for the read-only `mosaicfs_browser` CouchDB user, issued by Axum on successful login. Agents expose one port for P2P file transfers (default 7845), which should be accessible only within the local network.

For deployments where the control plane needs to be reachable from outside the home network — to support the web UI or CLI from a remote location — the recommended approach is a VPN (Tailscale is a natural fit) rather than exposing port 8443 directly to the internet. If direct internet exposure is unavoidable, the control plane should be placed behind a reverse proxy with rate limiting and, ideally, IP allowlisting.

### Known Gaps and Multi-User Considerations

The v1 security model makes several deliberate simplifications that would need to be revisited before MosaicFS could support multiple users with private, isolated file namespaces:

**Flat credential permissions.** Every credential grants full access to the entire system. The `permissions.scope` field in the credential document is reserved for future use but has no effect in v1. Adding per-user access control requires a permission model that the VFS layer, rule engine, transfer endpoints, and every API route would need to enforce.

**Global virtual directories.** All virtual directories and their mount configurations are shared across all credentials. In a multi-user system, different users would need different virtual trees. Adding per-user directory ownership is feasible in the schema, but requires changes to the rule engine (evaluate only directories the credential owns) and to the replication filters (agents replicate only the directory documents relevant to them).

**CouchDB replication is not per-credential.** Agents replicate directly with CouchDB using a shared internal credential managed by the control plane. The replication filter controls what documents travel, but any agent with replication access can pull any document that passes the filter. Proper per-user isolation would likely require moving agent synchronization behind the Axum API, which can enforce credential-scoped access — a meaningful architectural change from direct CouchDB replication.

**File transfers have no per-file authorization.** Any valid credential can request any file from any agent's transfer server. The transfer endpoint authenticates the caller but does not check whether that caller is permitted to access the specific file requested.

None of these are blockers for a single-user deployment. The credential schema, rule document structure, and node ownership patterns are designed to accommodate these extensions, but the work of implementing them is substantial and is deferred to a future version.

---

## Federation

Federation is the planned approach to multi-user support in MosaicFS. Rather than building per-user access control within a single instance — which requires rearchitecting the replication model, the rule engine, and every API endpoint — each user runs their own sovereign MosaicFS instance. Sharing between users is explicit and opt-in: an instance exposes a slice of its virtual namespace to a peer instance, and the peer mounts that slice into its own virtual tree.

This preserves the simplicity of the single-user security model within each instance while enabling sharing across instances. Federation is not implemented in v1, but the v1 design accommodates it with minimal forward-looking additions described below.

### The Sovereignty Model

A MosaicFS instance is fully self-contained. It controls its own files, its own rules, its own virtual namespace, and its own credentials. No external entity can read from or write to an instance without that instance explicitly agreeing to the relationship. A federated peer is not a participant in the local CouchDB replication topology — it is accessed only through the transfer API, at the discretion of the exporting instance.

This boundary means federation adds no new trust surface to an existing instance. An instance that has no peering agreements configured behaves identically to a v1 instance. Federation is purely additive.

### Export Modes

Three export modes are planned, forming a permission gradient from surgical to broad:

**Mode 1 — Virtual export rule.** The exporting instance creates a dedicated rule whose step pipeline filters exactly what should be shared with a named peer. The rule uses the same pipeline model as any other virtual mount rule — globs, age filters, MIME filters, and so on — but its `export` field identifies the peer instances it is visible to. This is the most precise sharing mechanism, appropriate for sharing a specific project folder with a collaborator.

**Mode 2 — Re-export of existing rule.** Rather than duplicating rule logic, an existing virtual mount rule is flagged for export by populating its `export.peer_ids` field. The rule's existing step pipeline determines what is visible; the export field makes that filtered view available to the named peers. Changes to the rule's steps are immediately reflected in what peers can see. This is appropriate when a rule already describes exactly the right set of files and writing a separate export rule would duplicate the logic.

**Mode 3 — Peering agreement.** A `peering_agreement` document establishes a broad sharing relationship between two instances. Rather than configuring exports per rule, the agreement defines what named exports or the entire virtual namespace are shared with the peer. This is appropriate for trusted peers — family members, a partner — where granular per-rule control is more friction than it is worth.

### How Federated Peers Map to Existing Concepts

From the receiving instance's perspective, a federated peer looks structurally similar to a cloud bridge node: it is a remote source of file metadata and file bytes, accessed via an HTTP endpoint, with its files appearing under a subtree of the local virtual namespace. The key differences are that the peer is another MosaicFS instance rather than a cloud API, and the relationship is governed by a peering agreement rather than an OAuth token.

This maps onto existing concepts cleanly:

- A federated peer is represented as a `node` document with `node_kind: "federated_peer"` — a planned value not used in v1 but reserved in the schema.
- Files imported from a peer are represented as `file` documents with `source.node_id` pointing to the federated peer node. The VFS tiered access system gains a new tier — "fetch from peer instance via transfer API" — sitting between the control plane bridge fetch and a future write tier.
- The virtual path prefix `/federation/` is reserved for imported peer namespaces. A peer named "alice" whose exported documents folder is mounted locally would appear at `/federation/alice/documents/`. This prefix is not used by local rules.
- The unified `export_path` field on rule sources works identically for local nodes and federated peers. A merge rule spanning both looks exactly like a merge rule spanning two local nodes:

```json
"sources": [
  { "node_id": "node-laptop",  "export_path": "/home/bob/documents" },
  { "node_id": "peer-alice",   "export_path": "/home/alice/documents" }
]
```

The rule engine resolves each source by asking the node what files live at that export path — physical agents answer with filesystem paths, federated peers answer with their virtual paths. The step pipeline then applies uniformly to all results.

### Cross-Instance Authentication

Authentication between instances uses instance-level keypairs rather than the shared credential model used within a single instance. Each MosaicFS instance generates an Ed25519 keypair at setup time. When two instances establish a peering agreement, they exchange public keys. Transfer requests from a peer instance are signed with the requesting instance's private key and verified against the stored public key — no per-user credentials are issued across the instance boundary.

This preserves the sovereignty model: instance A never issues credentials to instance B's users, and instance B never has direct access to instance A's CouchDB. The transfer endpoint on instance A simply validates that a request is signed by a known peer key and that the requested file is covered by an active peering agreement.

### Planned Document Types

Two new document types are planned for the federation implementation. They are not part of v1 but are designed here to ensure the v1 schema does not conflict with them.

**`peering_agreement`** — describes a bilateral sharing relationship. Lives on both instances. Contains the peer's instance ID, the peer's transfer endpoint, the peer's public key, what is shared (a list of export rule IDs, or `"all"` for the full virtual namespace), the direction of sharing (`"outbound"`, `"inbound"`, or `"bilateral"`), and the agreement status (`"pending"`, `"active"`, or `"suspended"`). An agreement begins in `"pending"` state and becomes `"active"` only when both instances have confirmed it — preventing one-sided peering.

**`federated_import`** — describes how an imported peer namespace is mounted into the local virtual tree. Lives on the receiving instance only. Contains the peer's instance ID, which of their exports to mount, the local virtual path prefix to mount it under, and a polling interval for metadata refresh. The VFS layer and rule engine treat an active `federated_import` as a read-only subtree source, similar to how embedded `network_mounts` entries on a node document drive tiered access.

### v1 Accommodations

The federation implementation itself is deferred, but three small additions to the v1 design ensure future compatibility without adding implementation complexity:

**`export` field on virtual directory mounts.** An optional `export` object is included in each mount entry schema. In v1 the rule engine ignores it entirely. Users who want to flag specific mounts for future export can populate it without any schema migration when federation ships.

```json
"export": {
  "enabled": false,
  "peer_ids": []
}
```

**`node_kind: "federated_peer"` reserved.** The `node_kind` field on node documents is documented to accept `"federated_peer"` as a future value. v1 components that encounter this value should treat the node as inactive rather than erroring. This allows federation-capable agents to be deployed alongside v1 agents without breaking the existing system.

**`/federation/` virtual path prefix reserved.** Local directories must not use `/federation/` as a path prefix. This prefix is reserved for imported peer namespaces. The Virtual Filesystem editor in the web UI will warn if a user attempts to create a directory at this path. A `mirror_source` mount strategy — where a federated peer's `export_path` is used verbatim as the local virtual path — is planned as an additional strategy alongside `prefix_replace` and `flatten`. Because a federated peer's `export_path` is already a virtual path, mirroring it locally requires no transformation.

---

## PART TWO — Technical Reference

This section contains the detailed technical specifications for the MosaicFS system: document schemas, data structures, protocols, and component interfaces. It is intended as a reference for implementors.

---

## Technology Stack

| Component | Technology | Notes |
|---|---|---|
| Agent daemon | Rust | Single static binary. Uses tokio for async, notify crate for filesystem watching. |
| VFS common layer | Rust (`mosaicfs-vfs`) | Shared library crate. Rule evaluation, tiered access, file cache (full-file and block modes), download deduplication. Used by all OS-specific backends. |
| FUSE backend (v1) | Rust / fuser | Implemented within the agent binary. Uses the `fuser` crate for FUSE bindings. Read-only in v1. Used on Linux and macOS (via macFUSE). |
| macOS File Provider (future) | Swift / FileProvider framework | Separate macOS app extension communicating with the agent via XPC. Provides native Finder integration, on-demand hydration, sync-state badges. |
| Windows CFAPI (future) | Rust / Windows crate | Desktop app component alongside the agent. Uses the Windows Cloud Files API (`cfapi.h`). Provides native File Explorer integration, placeholder files, hydration progress UI. |
| GIO / KIO backends (future) | C / Rust FFI | GVfs backend (GNOME) and KIO worker (KDE). Registers `mosaicfs://` URI scheme for desktop-aware applications. Calls the REST API or agent Unix socket; no kernel driver required. |
| Control plane API | Rust / Axum | Built on tokio + hyper. Serves both the REST API and static web UI assets. |
| Database | CouchDB 3 | Runs in Docker on the control plane host. Never exposed externally. |
| Agent local DB | Rust CouchDB client | Speaks the CouchDB replication protocol natively. |
| Web UI | React + Vite | Single-page application. Uses shadcn/ui components, TanStack Query for API calls. |
| Browser sync | PouchDB | Syncs directly with CouchDB as the `mosaicfs_browser` read-only user. Session token issued by Axum at login. Pull-only; push rejected at database level. |
| Deployment | Docker Compose | Control plane runs as a Compose stack. Agents install as systemd / launchd services. |

---

## Data Model Overview

All state in MosaicFS is stored as JSON documents in CouchDB and replicated between agents and the control plane. There are no separate relational tables or sidecar databases for core metadata — everything lives in one document store, which is what makes the replication model so clean. Understanding the document types and how they relate to each other is the key to understanding how the system works.

### Document Types at a Glance

MosaicFS uses twelve document types in v1, each with a distinct role in the system. Two additional types — `peering_agreement` and `federated_import` — are designed but not implemented; they are described in the Federation section.

| Document Type | `_id` Prefix | Purpose |
|---|---|---|
| `file` | `file::` | One document per indexed file. The core unit of the system. Carries real-world location (`source.node_id`, `source.export_path`). Virtual locations are computed on demand by the rule engine; not stored on the document. |
| `virtual_directory` | `dir::` | One document per directory in the virtual namespace. Explicitly created and managed by the user. Carries the directory's mount sources — the rules that define what files and subdirectories appear inside it. |
| `node` | `node::` | One document per participating device or cloud bridge. Describes the node, its transfer endpoint, storage topology, and embedded network mount declarations. |
| `credential` | `credential::` | Preshared access key pairs used by agents and the web UI to authenticate with the control plane. |
| `agent_status` | `status::` | Published periodically by each agent. Provides operational health data for the web UI dashboard. |
| `utilization_snapshot` | `utilization::` | Point-in-time record of storage capacity and usage for a node. Written hourly; used to compute utilization trends over time. |
| `label_assignment` | `label_file::` | Associates one or more user-defined labels with a specific file, identified by `node_id` and `export_path`. Written by the user via the API; never touched by the agent crawler. Survives file re-indexing. |
| `label_rule` | `label_rule::` | Applies one or more labels to all files under a given path prefix on a given node. Acts as an inherited label source: a file's effective label set is the union of its direct `label_assignment` labels and all `label_rule` labels whose prefix covers the file's `export_path`. |
| `plugin` | `plugin::` | Configuration for one plugin on one node. Specifies the plugin type (`executable` or `socket`), the plugin name (resolved to a binary in the node's plugin directory), subscribed events, MIME filter globs, worker count, timeout, and an arbitrary `config` object passed to the plugin at invocation time. Managed via the web UI; the agent watches the changes feed and reloads plugin configuration live. |
| `annotation` | `annotation::` | Structured metadata written back to CouchDB by executable plugins. One document per `(file, plugin_name)`. The plugin's entire stdout JSON object is stored verbatim in the `data` field. Socket plugins that update external systems typically produce no annotation documents. |
| `notification` | `notification::` | A system event or condition requiring user attention. Written by agents, cloud bridges, the control plane, and plugins. One document per distinct condition — identified by a stable `condition_key` so the same condition updates rather than duplicates. Carries severity, source, message, optional action links, and a lifecycle status (`active`, `resolved`, `acknowledged`). Replicated to the browser via PouchDB for live delivery without polling. |

### How the Document Types Relate

The relationships between document types reflect the layered architecture of the system. At the bottom is the physical layer: nodes own files on real filesystems. Above that is the virtual layer: virtual directories carry mount sources that define what files appear inside them, and the rule engine evaluates those sources on demand to answer directory listings. Connecting the two is the access layer: network mount documents let the VFS layer find the cheapest path to each file's bytes. Cutting across all layers is the label system: label assignments and label rules attach arbitrary user-defined tags to files, which the rule engine and search API can filter on.

A `file` document is a fact about a real file — where it lives and what it looks like. It has no knowledge of where it appears in the virtual tree, and it carries no labels directly. Labels are stored in separate `label_assignment` documents (keyed by node + path, never overwritten by the crawler) and `label_rule` documents (which apply labels to entire directory subtrees by path prefix). A file's effective label set — the union of its direct assignments and all prefix-matching rules — is computed at query time by the rule engine and search API.

`virtual_directory` documents are the primary configuration surface. A directory's `mounts` array describes what gets mounted inside it — each mount entry specifying a source, a filter step pipeline, and a mapping strategy. Directories are created and deleted explicitly by the user; they are not created automatically as a side effect of rules. An empty directory (one with no mounts) is a valid, persistent container for other directories.

### How Each Component Uses the Data Model

Different components of MosaicFS have distinct, non-overlapping write responsibilities. Understanding who writes what is important for reasoning about data consistency.

| Component | Writes | Reads |
|---|---|---|
| Agent crawler / watcher | `file`, `agent_status`, `utilization_snapshot`, `notification` (crawl events, watch limit, cache pressure) | `credential` (auth), node's `network_mounts` (path hints) |
| Agent plugin runner | `annotation` (from executable plugin stdout), `agent_status` (plugin subsystem health), `notification` (job failures, plugin health check results) | `plugin` (configuration), `file` (event payloads), `annotation` (stale check on re-crawl) |
| Rule evaluation engine (VFS layer / control plane) | Nothing — read-only evaluation | `file`, `virtual_directory` (mount sources + steps), `node`, `label_assignment`, `label_rule`, `annotation` |
| VFS backend (FUSE / File Provider / CFAPI) | Nothing — read-only in v1 | `file`, `virtual_directory` (readdir), node's `network_mounts`, `node` |
| Control plane API (Axum) | `credential`, `node` (registration, network_mounts), `virtual_directory`, `label_assignment`, `label_rule`, `plugin`, `notification` (system-level events, credential activity) | All document types |
| Cloud bridge runners (control plane) | `file` (cloud files), `node` (bridge docs), `agent_status`, `utilization_snapshot`, `notification` (OAuth expiry, quota warnings, sync failures) | node's `network_mounts` |
| Web UI (browser / PouchDB) | `virtual_directory` (via API), node's `network_mounts` (via API), `label_assignment` (via API), `label_rule` (via API), `plugin` (via API), `notification` (acknowledge via API) | All document types (via PouchDB live sync) |

### Replication Topology

Not all documents are replicated to all nodes. The replication topology is filtered to match each node's needs:

- **Physical agents** replicate `file`, `virtual_directory`, `node`, `credential`, `label_assignment`, `label_rule`, `plugin`, `annotation`, and `notification` documents bidirectionally with the control plane. Network mount declarations travel as part of the node document rather than as separate documents. This gives the VFS layer everything it needs to evaluate directory mount sources, resolve label sets, query annotations, and find file locations without a network round trip. Plugin configuration documents replicate to agents so the plugin runner can load them without contacting the control plane. Notification documents replicate bidirectionally so the browser receives notifications from agents in real time via PouchDB, and acknowledgements written by the browser via the REST API propagate back to the originating agent.
- **`agent_status`** is pushed from each agent to the control plane only — it is not replicated back out to other agents, since no agent needs to know the health of another agent directly.
- **The browser (PouchDB)** syncs a read-only subset of the database directly with the control plane's CouchDB instance, enabling live-updating UI without custom WebSocket infrastructure. `credential` documents are excluded from browser replication for security.

### Soft Deletes and Document Lifecycle

MosaicFS uses soft deletes for file documents rather than CouchDB's native deletion mechanism. When a file is removed from a node's filesystem, its document is updated with `status: "deleted"` and a `deleted_at` timestamp rather than being deleted outright. This preserves the inode number if the file reappears, ensures other nodes learn about the deletion through normal replication, and maintains a deletion history for debugging.

Virtual directory documents are explicitly created and deleted by the user. They are never created or tombstoned automatically. Node and credential documents are never deleted; they are disabled via a `status` or `enabled` flag to preserve the audit trail and prevent orphaned references.

### CouchDB Indexes

CouchDB Mango indexes are created at setup time — on the control plane during initial setup, and on each agent at first startup. Each index covers a specific query pattern used by one or more components. Without these indexes, Mango falls back to full collection scans, which are acceptable for very small deployments but degrade as the file count grows.

The **Location** column indicates where each index must exist. "Control plane" means the index is only needed on the central CouchDB instance. "Agent local" means the index must also be created on each agent's local CouchDB replica, because the VFS layer or agent-side authentication queries that replica directly without going through the control plane.

| Index Fields | Location | Used By | Purpose |
|---|---|---|---|
| `type`, `status` | Control plane + Agent local | Search API, VFS layer | The baseline filter applied to almost every query. Narrows the candidate set to active file documents before any further filtering. |
| `type`, `source.node_id`, `source.export_path` | Control plane + Agent local | Rule engine (`readdir`), VFS layer (`open`) | Resolves a node ID and export path to its file document. Used by the rule engine when evaluating mount sources and by the VFS layer to open files. Must be local so filesystem operations require no network round trip. |
| `type`, `source.node_id`, `source.export_parent` | Control plane + Agent local | Rule engine (`readdir`) | Lists all files under a given real directory on a specific node. Used when evaluating a `prefix_replace` mount source — fetches all files whose `export_parent` starts with the source path prefix. Must be local for VFS performance. |
| `type`, `source.node_id` | Control plane only | Nodes page | Fetches all files belonging to a specific node. Used by the web UI node detail page to show indexed file counts. Not needed locally — agents don't query other nodes' files. |
| `type`, `status`, `name` | Control plane only | Search API | Supports filename substring and glob search. The `type` and `status` fields narrow the scan to active file documents; the regex match on `name` is then applied to this reduced set. Search runs on the control plane only. |
| `type`, `inode` | Control plane + Agent local | VFS layer | Resolves an inode number back to a document. Used by FUSE operations that receive an inode rather than a path. Must be local so inode resolution requires no network round trip. |
| `type`, `node_id`, `captured_at` | Control plane only | Storage page, utilization trend charts | Queries utilization snapshots for a given node over a time range. Not replicated to agents; only the control plane and web UI query snapshot history. |
| `type`, `enabled` | Control plane + Agent local | Authentication middleware, agent-to-agent transfers | Looks up a credential document during request signing validation. Must be local on agents because transfer authentication between two agents is validated against the local replica without involving the control plane. |
| `type`, `status` (node docs) | Control plane only | Dashboard, health checks | Fetches all nodes with a given status. Used by the dashboard to render node health indicators and by the control plane's health check poller. |
| `type`, `node_id`, `export_path` | Control plane + Agent local | Rule engine (label step), Search API | Looks up a `label_assignment` document for a specific file by node + path. Used when computing a file's effective label set during step pipeline evaluation and label-based search. Must be local for VFS performance. |
| `type`, `node_id`, `path_prefix` | Control plane + Agent local | Rule engine (label step), Search API | Lists all `label_rule` documents that could cover a given file path on a given node. The rule engine loads all rules for the relevant node and checks which prefixes match. Must be local for VFS performance. |
| `type`, `node_id`, `export_path`, `plugin_name` | Control plane + Agent local | Rule engine (annotation step), Search API | Looks up an `annotation` document for a specific file and plugin. Used during step pipeline evaluation for the `annotation` op and during annotation-based search. Must be local for VFS performance. |
| `type`, `node_id` (plugin docs) | Control plane + Agent local | Agent plugin runner | Fetches all enabled plugin configurations for a given node. The agent loads this on startup and reloads on changes feed updates. Must be local so the plugin runner does not require a control plane round trip at startup. |
| `type`, `status`, `severity` (notification docs) | Control plane only | Notification API, dashboard | Fetches active and unacknowledged notifications sorted by severity for the notification panel and dashboard alert area. The browser receives notification documents via PouchDB live sync and filters client-side, but the REST API uses this index for server-side queries. |

A note on `$regex` queries: CouchDB Mango does not support true text indexes — `$regex` always performs a scan of the candidate set after index filtering. For filename search this means the `type` + `status` + `name` index reduces the scan to active file documents, but the regex itself is evaluated in memory on the control plane. This is acceptable at home-deployment scale. If search performance degrades as the file count grows, the correct solution is a dedicated search engine rather than further CouchDB index tuning.

### Replication Flows

CouchDB replication between agents and the control plane is filtered — each flow carries only the documents the destination actually needs. This keeps agent replicas lean and avoids leaking sensitive documents (credentials, utilization history) to nodes that have no use for them. Filters are expressed as Mango selectors attached to CouchDB replication documents.

There are three network replication flows. Cloud bridges are a fourth logical flow but are internal to the control plane process — bridges write directly to the local CouchDB instance, and those writes fan out to agents via Flow 2.

**Flow 1 — Agent → Control Plane (push)**

Each agent pushes only the documents it owns or that the user has created locally. It never pushes documents it received from the control plane back upstream.

```json
{
  "$or": [
    { "type": "file",                 "source.node_id": "<this_node_id>" },
    { "type": "node",                 "_id":            "node::<this_node_id>" },
    { "type": "agent_status",         "node_id":        "<this_node_id>" },
    { "type": "utilization_snapshot", "node_id":        "<this_node_id>" },
    { "type": "label_assignment",     "node_id":        "<this_node_id>" },
    { "type": "label_rule",           "node_id":        "<this_node_id>" },
    { "type": "annotation",           "node_id":        "<this_node_id>" },
    { "type": "notification",         "source.node_id": "<this_node_id>" }
  ]
}
```

**Flow 2 — Control Plane → Agent (pull)**

The agent pulls everything the VFS layer and local authentication need to operate without contacting the control plane. Network mount declarations travel as part of the node document. Only disabled credentials are excluded to keep the local replica clean.

```json
{
  "$or": [
    { "type": "file" },
    { "type": "virtual_directory" },
    { "type": "node" },
    { "type": "credential",       "enabled": true },
    { "type": "label_assignment" },
    { "type": "label_rule" },
    { "type": "plugin" },
    { "type": "annotation" },
    { "type": "notification" }
  ]
}
```

The following document types are deliberately excluded from agent replicas: `agent_status` (agents don't monitor each other), `utilization_snapshot` (history only needed by the control plane and web UI). Plugin documents for *other* nodes are excluded — each agent only needs its own node's plugin configurations, which arrive via the node-scoped filter in Flow 1. Control-plane-originated notification documents (OAuth expiry, system-level alerts) replicate to agents via this flow so the browser receives them through the same PouchDB channel regardless of origin.

**Flow 3 — Control Plane → Browser (PouchDB pull)**

The browser authenticates to CouchDB as the `mosaicfs_browser` user — a restricted CouchDB role created during control plane setup. This user has read-only access to the `mosaicfs` database, enforced by CouchDB's own permission model. Push attempts from a browser client are rejected at the database level regardless of what the replication filter says — a hijacked session cannot write to the database.

The Axum login endpoint issues a short-lived CouchDB session token for `mosaicfs_browser` alongside the JWT used for REST API calls. PouchDB authenticates directly with this session token. Both tokens are held in memory only and are never written to `localStorage` or cookies.

The browser replication filter excludes documents the browser has no need for:

```json
{
  "$or": [
    { "type": "file",               "status": "active" },
    { "type": "virtual_directory" },
    { "type": "node" },
    { "type": "agent_status" },
    { "type": "label_assignment" },
    { "type": "label_rule" },
    { "type": "plugin" },
    { "type": "annotation" },
    { "type": "notification" }
  ]
}
```

`credential` documents are excluded — the browser never needs to see secret key hashes, and the `mosaicfs_browser` role does not have read access to them even if the filter were misconfigured. `utilization_snapshot` documents are excluded because the browser fetches snapshot history on demand via the REST API rather than syncing the full time series into PouchDB.

**Deleted file tombstone propagation**

Excluding `status: "deleted"` files from agent replication creates a subtle problem: if a file is deleted on node A, agents on other nodes never receive the updated document and their VFS backends continue to list the deleted file indefinitely. The v1 approach sidesteps this by replicating deleted file documents to agents without filtering on `status` — accepting a modestly larger agent replica in exchange for correct deletion propagation. Deleted files are excluded at query time by the `status: "active"` condition applied in rule engine evaluation. The flow 2 filter above reflects this: the `file` selector omits `status: "active"` intentionally, so both active and deleted file documents replicate to agents.

When a file document transitions to `status: "deleted"`, it immediately drops out of any directory listing on the next `readdir` evaluation — the rule engine's step pipeline checks `status: "active"` before evaluating mount steps, so deleted files never appear as virtual directory contents regardless of what the mount sources say.

---

## CouchDB Document Schemas

All data in MosaicFS is stored as JSON documents in CouchDB. Each document type has a `type` field that identifies its role in the system.

### File Document

Represents a single file on a physical node or cloud service. Created and updated by the agent crawler and watcher. Carries only intrinsic properties of the file — where it physically lives and what it looks like. Virtual locations are computed on demand by evaluating virtual directory mount sources; they are not stored on the file document.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"file::{node_id}::{uuid}"`. The UUID is generated at document creation time. Unique across the system. |
| `type` | string | Always `"file"`. |
| `inode` | uint64 | Random 64-bit integer assigned at creation time. Stable for the lifetime of the file. Used as the inode number by the FUSE backend, and as the equivalent stable identity token by other VFS backends. A file appearing in multiple virtual directories presents the same inode in each — the OS treats these as hard links. |
| `name` | string | Filename component only (no directory path). |
| `source.node_id` | string | ID of the node that owns this file. |
| `source.export_path` | string | The path this node uses to identify this file. For physical agents: absolute filesystem path. For cloud bridges: path within the cloud service namespace. For federated peers: virtual path on the peer instance. |
| `source.export_parent` | string | Parent directory component of `export_path`. Used by the rule engine when evaluating `prefix_replace` mount sources — enables efficient lookup of all files under a given real directory. |
| `size` | uint64 | File size in bytes. |
| `mtime` | string | ISO 8601 last-modified timestamp. |
| `mime_type` | string? | MIME type if determinable. |
| `status` | string | `"active"` or `"deleted"`. Soft deletes preserve history. |
| `deleted_at` | string? | ISO 8601 timestamp if `status` is `"deleted"`. |

### Virtual Directory Document

Represents a directory in the virtual filesystem namespace. Virtual directories are the primary configuration surface in MosaicFS — each directory carries a `mounts` array that defines what files and subdirectories appear inside it. Directories are created and deleted explicitly by the user; they are never created or removed automatically.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"dir::sha256({virtual_path})"`. Deterministic — enables idempotent creation. |
| `type` | string | Always `"virtual_directory"`. |
| `inode` | uint64 | Random 64-bit integer. Inode 1 is reserved for the root directory. |
| `virtual_path` | string | Full path in the virtual namespace, e.g. `"/documents/work"`. |
| `name` | string | Directory name component only. |
| `parent_path` | string? | Parent virtual path. Null for the root directory. |
| `system` | bool? | True for the root and other well-known synthetic entries. Prevents accidental deletion. |
| `created_at` | string | ISO 8601 creation timestamp. |
| `enforce_steps_on_children` | bool | Default `false`. When `true`, this directory's own step pipeline (if any) is prepended to the evaluation of every mount in every descendant directory. Children can add further steps but cannot override or bypass ancestor steps. |
| `mounts` | array | Ordered list of mount sources. Each entry defines a source of files or subdirectories to mount into this directory. See mount entry fields below. |
| `mounts[].mount_id` | string | Short random identifier for this mount entry. Used by the API to target a specific mount for update or deletion. |
| `mounts[].source` | object | Source descriptor. Either `{node_id, export_path}` for a local or cloud node, or `{federated_import_id}` for a federated peer. `node_id` may be `"*"` to match all nodes. |
| `mounts[].strategy` | string | `"prefix_replace"` or `"flatten"`. `prefix_replace` strips the source prefix and mounts the remaining path hierarchy as a subtree. `flatten` places all matching files directly in this directory, discarding subdirectory structure. |
| `mounts[].source_prefix` | string? | Path prefix to strip from `export_path`. Required for `prefix_replace`. |
| `mounts[].steps` | array | Ordered filter steps. Same schema as before — `op`, `invert`, `on_match`, and op-specific parameters. Evaluated after any inherited ancestor steps. |
| `mounts[].default_result` | string | Default `"include"`. Result if all steps complete without a short-circuit. |
| `mounts[].conflict_policy` | string | `"last_write_wins"` or `"suffix_node_id"`. Applied when two sources produce a file at the same name within this directory. |

**Inheritance.** When `enforce_steps_on_children` is `true` on an ancestor directory, its steps are prepended to every mount evaluation in all descendant directories — from outermost ancestor to nearest parent, in that order. A child directory's own mount steps are appended last. This means ancestor steps evaluate first and cannot be bypassed: a child can narrow a parent's results further but cannot surface files the parent has excluded.

**Multiple appearances.** A file may appear in multiple virtual directories simultaneously. The rule engine evaluates each directory's mounts independently — there is no global deduplication. A `proposal.docx` modified yesterday might satisfy both a "Recent documents" directory (matched by an age step) and a "Work documents" directory (matched by a path source). Both are valid virtual locations for the same file. The file document's `inode` is the same in both listings, so the OS treats the two directory entries as hard links to the same file.

### Node Document

Represents a device or cloud bridge participating in the MosaicFS network. For physical nodes, the `storage` array describes the filesystem and disk topology visible to the agent. For cloud bridge nodes, the `cloud_storage` object captures quota and billing model. Point-in-time utilization figures are recorded in separate `utilization_snapshot` documents rather than here, keeping the node document stable and the snapshot history queryable.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"node::{node_id}"`. |
| `type` | string | Always `"node"`. |
| `node_kind` | string | `"physical"`, `"cloud_bridge"`, or `"federated_peer"` (reserved for federation, not used in v1). v1 components encountering `"federated_peer"` should treat the node as inactive. |
| `role` | string? | Optional. `"bridge"` for bridge nodes — agents with no watch paths whose filesystem is provided by plugins. Null for standard physical nodes. Used by the web UI to render bridge-specific controls on the node detail page. Has no effect on agent behavior; bridge nodes are operationally identical to physical nodes except for their plugin configuration. |
| `friendly_name` | string | Human-readable display name, e.g. `"MacBook Pro"`. |
| `platform` | string | `"linux"`, `"darwin"`, or `"windows"`. |
| `status` | string | `"online"`, `"offline"`, or `"degraded"`. |
| `last_heartbeat` | string | ISO 8601 timestamp of last heartbeat. |
| `vfs_capable` | bool | Whether this node can run a virtual filesystem backend. True for physical nodes with a supported OS. Used by the web UI to indicate which nodes support the filesystem mount. |
| `vfs_backend` | string? | The VFS backend active on this node, if any: `"fuse"`, `"file_provider"`, or `"cfapi"`. Null if the VFS layer is not running. |
| `capabilities` | string[] | Advertised query capabilities currently active on this node. Values are well-known strings: `"search"` indicates the node can service search queries. Updated dynamically by the agent as plugins come online and go offline — a socket plugin that disconnects removes its capability until it reconnects. The control plane uses this field to route `POST /api/query` requests; the UI uses it to discover what query types are available without knowing which specific plugins are installed. |
| `transfer.endpoint` | string | Host:port for direct P2P file transfer. |
| `transfer.protocol` | string | Always `"http"` in v1. |
| `bridge_type` | string? | For cloud_bridge nodes: `"google_drive"`, `"onedrive"`, `"s3"`, `"b2"`, `"icloud"`. |
| `owner` | string? | For cloud_bridge nodes: always `"control_plane"`. |
| `storage` | array? | Physical nodes only. Array of filesystem entries, one per filesystem containing watched paths. Refreshed hourly by the agent. |
| `storage[].filesystem_id` | string | Stable identifier for this filesystem. UUID from `blkid` on Linux, `diskutil` on macOS. |
| `storage[].mount_point` | string | Mount point of the filesystem, e.g. `"/"` or `"/mnt/data"`. |
| `storage[].fs_type` | string | Filesystem type: `"ext4"`, `"xfs"`, `"apfs"`, `"zfs"`, `"ntfs"`, etc. |
| `storage[].device` | string | Block device path, e.g. `"/dev/sda1"` or `"/dev/mapper/vg0-root"`. |
| `storage[].capacity_bytes` | uint64 | Total capacity of the filesystem in bytes. |
| `storage[].used_bytes` | uint64 | Used bytes at last agent refresh. Current snapshot figures live in `utilization_snapshot`. |
| `storage[].watch_paths_on_fs` | string[] | MosaicFS watch paths that reside on this filesystem. |
| `storage[].volume` | object? | Present when the filesystem sits on a logical volume. Contains `type` (`"lvm"`, `"zfs"`, `"apfs_container"`), volume group or pool name, logical volume name, and VG/pool total and free bytes. |
| `storage[].disk` | object? | Present when the underlying physical disk is identifiable. Contains device path, vendor, model, serial number, capacity in bytes, and interface type (`"nvme"`, `"sata"`, `"usb"`, etc.). |
| `cloud_storage` | object? | Cloud bridge nodes only. Describes the storage model for this cloud service. |
| `cloud_storage.billing_model` | string | `"quota"` for services with a fixed storage limit (Google Drive, OneDrive). `"consumption"` for pay-per-byte services with no ceiling (S3, B2). |
| `cloud_storage.quota_bytes` | uint64? | Total purchased storage in bytes. Present only when `billing_model` is `"quota"`. |
| `cloud_storage.quota_available` | bool | Whether quota data could be retrieved from the cloud API. False for iCloud, where no official quota API exists. |
| `network_mounts` | array? | Physical nodes only. Records network and cloud filesystems already mounted locally on this node. Used by the VFS layer's tiered access system to avoid redundant data transfer. Managed via the API; not collected automatically by the agent. |
| `network_mounts[].mount_id` | string | Short random identifier for this mount entry. Used by the API to target a specific mount for update or deletion. |
| `network_mounts[].remote_node_id` | string | The MosaicFS node whose files are accessible via this mount. |
| `network_mounts[].remote_base_export_path` | string | The base export path on the remote node that this mount covers. Matched against `source.export_path` values when the VFS layer resolves tiered access. |
| `network_mounts[].local_mount_path` | string | The local path at which the remote filesystem is mounted on this node. |
| `network_mounts[].mount_type` | string | `"cifs"`, `"nfs"`, `"gdrive_local"`, `"icloud_local"`, etc. |
| `network_mounts[].priority` | int | Higher values preferred when multiple mounts could serve the same file. |

**Example virtual directory** — a "Recent work documents" directory that mounts `.pdf`, `.docx`, and `.md` files from the laptop's documents folder, modified within the last 90 days, excluding anything under an `archive` subdirectory, but always including files with `URGENT` in the name. The parent `/documents` directory has `enforce_steps_on_children: true` with a step that excludes `.tmp` files globally — this is automatically prepended to the evaluation here.

```json
{
  "_id": "dir::sha256(/documents/work)",
  "type": "virtual_directory",
  "virtual_path": "/documents/work",
  "name": "work",
  "parent_path": "/documents",
  "inode": 4821,
  "created_at": "2025-11-14T09:00:00Z",
  "enforce_steps_on_children": false,
  "mounts": [
    {
      "mount_id": "a3f9",
      "source": { "node_id": "node-laptop", "export_path": "/home/user/documents" },
      "strategy": "prefix_replace",
      "source_prefix": "/home/user/documents",
      "steps": [
        { "op": "glob",  "pattern": "**/*.{pdf,docx,md}" },
        { "op": "glob",  "pattern": "**/archive/**", "invert": true },
        { "op": "regex", "pattern": "URGENT", "on_match": "include" },
        { "op": "age",   "max_days": 90 }
      ],
      "default_result": "include",
      "conflict_policy": "last_write_wins"
    }
  ]
}
```

When the VFS layer evaluates this directory, the parent's `.tmp` exclusion step is prepended first, then the mount's own steps run in sequence. Step 3 short-circuits any file with `URGENT` in the name directly to `include`, bypassing the age check. Files passing all steps without short-circuiting are included by `default_result`.

### Credential Document

Stores authentication credentials for agents and the web UI. Secret keys are stored as Argon2id hashes and are never recoverable after creation.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"credential::{access_key_id}"`. |
| `type` | string | Always `"credential"`. |
| `access_key_id` | string | Public identifier, format: `"MOSAICFS_{16_hex_chars}"`. Safe to log. |
| `secret_key_hash` | string | Argon2id hash of the secret key. Format: `"argon2id:$argon2id$..."`. |
| `name` | string | Human-readable label, e.g. `"Main laptop agent"`. |
| `enabled` | bool | Disabled credentials are rejected. |
| `created_at` | string | ISO 8601 creation timestamp. |
| `last_seen` | string? | ISO 8601 timestamp of last successful authentication. |
| `permissions.scope` | string | Always `"full"` in v1. Reserved for future scoped permissions. |

### Agent Status Document

Published by each agent on a regular schedule. Provides a rich operational picture of each node for the web UI status dashboard.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"status::{node_id}"`. |
| `type` | string | Always `"agent_status"`. |
| `node_id` | string | The node this status document describes. |
| `updated_at` | string | ISO 8601 timestamp of last update. |
| `overall` | string | `"healthy"`, `"degraded"`, or `"unhealthy"`. |
| `subsystems` | object | Per-subsystem status objects: `crawler`, `watcher`, `replication`, `cache`, `transfer`. |
| `recent_errors` | array | Last 50 errors, each with `time`, `subsystem`, `level`, and `message` fields. |

### Utilization Snapshot Document

A point-in-time record of storage capacity and usage, written hourly by each agent and cloud bridge runner. Snapshots are never updated in place — each hour produces a new document. This makes utilization history queryable using CouchDB key-range queries on the timestamp component of the `_id`, and means there is no contention between the agent writing snapshots and the control plane or web UI reading them. Snapshots older than 90 days are pruned by a background task on the control plane.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"utilization::{node_id}::{ISO8601_timestamp}"`. Timestamp truncated to the hour, e.g. `"utilization::node-laptop::2025-11-14T09:00:00Z"`. |
| `type` | string | Always `"utilization_snapshot"`. |
| `node_id` | string | The node this snapshot describes. |
| `captured_at` | string | ISO 8601 timestamp when the snapshot was taken. |
| `filesystems` | array? | Physical nodes only. One entry per filesystem. |
| `filesystems[].filesystem_id` | string | Matches the `filesystem_id` in the node document `storage` array. Used to join snapshots to their filesystem topology. |
| `filesystems[].mount_point` | string | Mount point at time of capture. Included for readability; `filesystem_id` is the stable join key. |
| `filesystems[].used_bytes` | uint64 | Bytes used on this filesystem at time of capture. |
| `filesystems[].available_bytes` | uint64 | Bytes available at time of capture. Note: used + available may be less than capacity due to reserved blocks. |
| `cloud` | object? | Cloud bridge nodes only. |
| `cloud.used_bytes` | uint64 | Bytes consumed in the cloud service at time of capture. |
| `cloud.quota_bytes` | uint64? | Total quota in bytes. Omitted for consumption-billed services (S3, B2). |

---

### Label Assignment Document

Associates one or more user-defined labels with a specific file. Created and updated via the API; the agent crawler never reads or writes this document type. Label assignments survive file re-indexing — if a file is modified and its `file` document is rewritten by the crawler, the corresponding `label_assignment` document is unaffected.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"label_file::{node_id}::{sha256(export_path)}"`. Deterministic — one document per file per node. Enables idempotent creation and upsert. |
| `type` | string | Always `"label_assignment"`. |
| `node_id` | string | ID of the node that owns the file. |
| `export_path` | string | The file's `source.export_path` on that node. |
| `labels` | string[] | Ordered array of label strings. Labels are arbitrary user-defined strings. No central registry — a label exists when something references it. |
| `updated_at` | string | ISO 8601 timestamp of last modification. |
| `updated_by` | string | Access key ID of the credential that last wrote this document. |

### Label Rule Document

Applies one or more labels to all files whose `source.export_path` starts with a given prefix on a given node. This is the mechanism behind "apply labels to all files in this folder and its subdirectories." A label rule is a declaration, not a bulk write — it does not modify individual file documents. The rule engine and search API compute a file's effective label set at query time by taking the union of its `label_assignment` labels and all `label_rule` labels whose `path_prefix` covers the file's `export_path`.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"label_rule::{uuid}"`. UUID assigned at creation. |
| `type` | string | Always `"label_rule"`. |
| `node_id` | string | ID of the node this rule applies to. May be `"*"` to apply to files from all nodes (useful for cross-node label rules). |
| `path_prefix` | string | Path prefix to match against `source.export_path`. A file matches if its path starts with this prefix. Must end with `/` to avoid partial directory name matches (e.g. `"/home/mark/documents/"` not `"/home/mark/documents"`). |
| `labels` | string[] | Labels to apply to all matching files. |
| `name` | string | Human-readable description shown in the web UI (e.g. `"Work documents"`). |
| `enabled` | bool | Disabled rules are ignored by the rule engine and search API. |
| `created_at` | string | ISO 8601 creation timestamp. |

**Effective label set.** Given a file document, the effective label set is computed as:

```
effective_labels(file)
  → result = {}
  → fetch label_assignment where node_id = file.node_id AND export_path = file.export_path
      if found: result ∪= assignment.labels
  → fetch all label_rules where node_id IN [file.node_id, "*"] AND enabled = true
      for each rule where file.export_path starts with rule.path_prefix:
          result ∪= rule.labels
  → return result
```

This is computed on demand during step pipeline evaluation and search. The two CouchDB indexes on `(type, node_id, export_path)` for assignments and `(type, node_id, path_prefix)` for rules make both lookups efficient.

---

### Plugin Document

Configures one plugin on one agent node. Created and managed via the web UI and REST API. The agent watches the CouchDB changes feed for documents matching its own `node_id` and reloads plugin configuration live — no restart required. Changes to a plugin document while jobs are in flight complete with the previous configuration; the updated configuration takes effect for subsequent jobs.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"plugin::{node_id}::{plugin_name}"`. Deterministic — one document per plugin per node. |
| `type` | string | Always `"plugin"`. |
| `node_id` | string | ID of the agent node this plugin runs on. |
| `plugin_name` | string | Name of the plugin executable. Resolved to the platform-specific plugin directory at invocation time. Must match an executable present in that directory; if the binary is absent, jobs fail immediately with a permanent error. |
| `plugin_type` | string | `"executable"` or `"socket"`. Determines the invocation model. |
| `enabled` | bool | Disabled plugins receive no events and enqueue no jobs. |
| `name` | string | Human-readable display name shown in the web UI (e.g. `"AI Document Summariser"`). |
| `subscribed_events` | string[] | Events this plugin receives. Valid values: `"file.added"`, `"file.modified"`, `"file.deleted"`, `"sync.started"`, `"sync.completed"`, `"crawl_requested"` (bridge nodes only — delivered instead of a filesystem crawl), `"materialize"` (bridge nodes only — delivered when a file under `file_path_prefix` is not on disk and must be synthesized). A plugin that does not subscribe to an event type never receives it. |
| `mime_globs` | string[] | Optional MIME type filter. If non-empty, only files whose `mime_type` matches at least one glob are enqueued. e.g. `["application/pdf", "text/*"]`. Files with no `mime_type` do not match. |
| `config` | object | Arbitrary JSON object passed to the plugin in the `config` field of every event's stdin payload. The plugin reads whatever keys it needs; extra keys are ignored. |
| `workers` | int | Number of concurrent workers for this plugin. Default 2. For executable plugins, this is the number of simultaneous child processes. Socket plugins use a single connection with a sliding acknowledgement window. |
| `timeout_s` | int | Maximum seconds to wait for a plugin response before treating the invocation as failed. Default 60. |
| `max_attempts` | int | Maximum number of attempts before a job is moved to permanent failure state. Default 3. Does not apply to socket plugins — delivery is retried indefinitely via the ack queue until the socket reconnects. |
| `query_endpoints` | array? | Optional. Declares query endpoints this plugin handles. Each entry causes the agent to advertise a capability on the node document and accept query requests routed by the control plane. See query endpoint fields below. |
| `query_endpoints[].name` | string | Internal endpoint name, used in routing. e.g. `"search"`. |
| `query_endpoints[].capability` | string | The well-known capability string this endpoint satisfies. Defined capability values: `"search"` — the plugin handles text search queries dispatched by the browser via `POST /api/query`; `"dashboard_widget"` — the plugin provides a periodic health/status summary polled by the control plane and displayed as a widget card on the dashboard. Multiple plugins on the same node may advertise the same capability — their results are merged in the response. |
| `query_endpoints[].description` | string | Human-readable description shown in the UI. e.g. `"Full-text search powered by Meilisearch"`. |
| `settings_schema` | object? | Optional. A JSON Schema-subset object declaring the user-configurable settings for this plugin. When present, the web UI renders a settings form on the Settings page rather than a raw JSON editor. Each property in `settings_schema.properties` describes one field with `type` (`"string"`, `"number"`, `"boolean"`, `"enum"`), `title` (label), `description` (help text), `default`, and for enum fields an `enum` array of permitted values. A `"secret"` type is also supported — rendered as a password input, displayed as `••••••••` after save. Required fields are listed in `settings_schema.required`. |
| `settings` | object? | User-provided values for the fields declared in `settings_schema`. Written by the web UI when the user saves the settings form. The agent merges `settings` into `config` at invocation time — the plugin binary receives a single flat `config` object and does not need to know which keys came from `settings` and which from `config`. If a key appears in both, `settings` takes precedence. |
| `provides_filesystem` | bool? | Default `false`. When `true`, this plugin acts as the filesystem for the node — the agent invokes it for crawl events instead of walking real watch paths, and for materialize events when the transfer server needs to serve a file whose `export_path` falls under `file_path_prefix`. Only meaningful on bridge nodes where `agent.toml` declares no watch paths. |
| `file_path_prefix` | string? | Required when `provides_filesystem` is `true`. Export path prefix identifying files owned by this plugin, e.g. `"/gmail"`. The transfer server checks whether a requested file's `export_path` starts with this prefix to decide whether to invoke the plugin's materialize action. Must be unique across all plugins on a given node. |
| `created_at` | string | ISO 8601 creation timestamp. |

**Plugin directory paths by platform:**

| Platform | Path |
|---|---|
| Linux | `/usr/lib/mosaicfs/plugins/` |
| macOS | `/Library/Application Support/MosaicFS/plugins/` |
| Windows | `C:\ProgramData\MosaicFS\plugins\` |

The agent enumerates this directory at startup and after each `inotify`/`FSEvents` change to the directory, reporting available plugin names in `agent_status.available_plugins`. The web UI uses this list to populate the plugin name dropdown when creating a plugin configuration.

**Event envelope (stdin for executable plugins; framed JSON over socket for socket plugins):**

The agent merges `settings` into `config` before constructing the envelope — `settings` values take precedence over same-named keys in `config`. The plugin binary receives a single flat `config` object and does not need to distinguish between the two sources.

```json
{
  "event":      "file.added",
  "sequence":   1042,
  "timestamp":  "2026-02-16T09:22:00Z",
  "node_id":    "node-laptop",
  "payload": {
    "file_id":      "file::node-laptop::a3f9...",
    "export_path":  "/home/mark/documents/report.pdf",
    "name":         "report.pdf",
    "size":         204800,
    "mime_type":    "application/pdf",
    "mtime":        "2026-01-15T14:30:00Z"
  },
  "config": {
    "meilisearch_url": "http://meilisearch:7700",
    "meilisearch_api_key": "abc123",
    "max_results": 20
  }
}
```

**Example `settings_schema` and `settings` for the fulltext-search plugin:**

```json
"settings_schema": {
  "properties": {
    "meilisearch_url": {
      "type": "string",
      "title": "Meilisearch URL",
      "description": "URL of the Meilisearch instance, e.g. http://meilisearch:7700",
      "default": "http://meilisearch:7700"
    },
    "meilisearch_api_key": {
      "type": "secret",
      "title": "Meilisearch API Key",
      "description": "Master key or search API key. Leave blank if authentication is disabled."
    },
    "max_results": {
      "type": "number",
      "title": "Maximum results per query",
      "default": 20
    }
  },
  "required": ["meilisearch_url"]
},
"settings": {
  "meilisearch_url": "http://meilisearch:7700",
  "meilisearch_api_key": "abc123",
  "max_results": 20
}
```

For `sync.started` and `sync.completed`, `payload` contains `{ "trigger": "manual" | "scheduled" }` and no file fields. For `file.deleted`, `payload` contains the file's last-known metadata.

**Bridge node events (`provides_filesystem: true` plugins only):**

Two additional event types are delivered exclusively to filesystem-providing plugins on bridge nodes.

`crawl_requested` — delivered by the agent on startup, on the nightly crawl schedule, and when the user triggers a manual sync. The plugin fetches new data from its external source, writes files to its `bridge-data/files/` directory, and returns a list of file operations for the agent to apply to CouchDB. `payload` contains `{ "trigger": "startup" | "scheduled" | "manual" }`.

Plugin stdout for `crawl_requested`:
```json
{
  "files": [
    {
      "action": "create",
      "export_path": "/gmail/2026/02/16/re-project-kickoff.eml",
      "size": 45231,
      "mtime": "2026-02-16T09:15:00Z",
      "mime_type": "message/rfc822"
    },
    {
      "action": "delete",
      "export_path": "/gmail/2026/01/10/old-newsletter.eml"
    }
  ]
}
```

The agent processes this list and applies it to CouchDB via `_bulk_docs` — creating, updating, or soft-deleting file documents as indicated. Files listed as `create` that already have a current document (same `mtime` and `size`) are skipped. This makes the crawl response idempotent: the plugin can safely return the full set of known files rather than only the delta.

`materialize` — delivered by the agent's transfer server when a file under `file_path_prefix` is requested and not present in the VFS cache. The plugin extracts the file from its internal storage (SQLite database, API response, etc.) and writes it to a staging path provided by the agent. The agent then takes over: moves the staged file into the VFS cache, inserts a cache index entry, and serves the bytes using the standard path. The plugin is responsible only for writing the bytes to disk — all cache management, integrity checking, and streaming is handled by the agent.

`materialize` stdin payload:
```json
{
  "event":        "materialize",
  "file_id":      "file::bridge-email-01::abc123",
  "export_path":  "/gmail/2026/02/16/re-project-kickoff.eml",
  "staging_path": "/var/lib/mosaicfs/cache/tmp/plugin-abc123",
  "config":       { }
}
```

Plugin stdout for `materialize`:
```json
{ "size": 45231 }
```

The plugin writes the file bytes to `staging_path` and returns the byte count. If materialization fails (message deleted from external source, authentication error), it exits non-zero with an error message on stderr. The agent logs the failure, removes the partial staging file, and returns a 503 to the requester. No retry — the next access will attempt materialization again.

**Executable plugin stdout contract:**

The plugin writes a single JSON object to stdout and exits 0 for success, non-zero for failure. Any top-level key in the returned object is written into the `annotation` document's `data` field. If the plugin has nothing to write back (it updated an external system), it returns `{}`. Malformed JSON or a non-zero exit is treated as a transient failure and retried up to `max_attempts`.

```json
{ "summary": "Quarterly earnings report for Q3 2025.", "language": "en" }
```

**Socket plugin ack protocol:**

The agent writes newline-delimited JSON events to the socket, each containing a `sequence` number. The plugin responds with newline-delimited JSON acks:

```json
{ "ack": 1042 }
```

The agent maintains a sliding window of unacknowledged events in the SQLite job queue. On socket disconnect, the agent retries the connection with exponential backoff and replays all unacknowledged events in sequence order after reconnecting. Socket plugins must be idempotent — they will receive duplicate events after a reconnect.

**Query invocation (executable plugins with `query_endpoints`):**

When the control plane routes a query to an agent, the agent invokes the plugin binary with the query payload on stdin and reads the result from stdout synchronously. This is request/response, not fire-and-forget. The plugin must respond within `timeout_s`.

Query stdin payload:
```json
{
  "query":    "quarterly earnings",
  "endpoint": "search",
  "config":   { }
}
```

Query stdout response — a result envelope identifying the plugin and containing an array of results:
```json
{
  "plugin_name":  "fulltext-search",
  "capability":   "search",
  "description":  "Full-text search powered by Meilisearch",
  "results": [
    {
      "file_id":   "file::node-laptop::a3f9...",
      "score":     0.94,
      "fragments": ["...quarterly **earnings** report for Q3..."]
    }
  ]
}
```

Each result may be a MosaicFS file reference (identified by `file_id`, looked up in PouchDB by the browser to display standard file metadata) or a free-form item (no `file_id`, rendered generically). The `fragments` field carries matched text snippets for search results. Additional fields are allowed and rendered as supplementary metadata.

**Dashboard widget response (`capability: "dashboard_widget"`):**

Polled by the control plane on a schedule rather than dispatched by the browser. The plugin returns a compact status summary:

```json
{
  "plugin_name":  "fulltext-search",
  "capability":   "dashboard_widget",
  "description":  "Full-text search powered by Meilisearch",
  "status":       "healthy",
  "data": {
    "Documents indexed": "47,203",
    "Index lag":         "0",
    "Last sync":         "2 minutes ago"
  }
}
```

`status` is `"healthy"`, `"warning"`, or `"error"` — controls the widget card's visual treatment. `data` is an ordered set of key-value pairs rendered as a compact list in the widget card. Values are strings; the plugin is responsible for human-readable formatting.

---

### Annotation Document

Structured metadata written back to CouchDB by an executable plugin. One document per `(file, plugin_name)` — rerunning the plugin for the same file overwrites the previous annotation. Socket plugins that update external systems (a search engine, an external database) typically produce no annotation documents; their output is their external system.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"annotation::{node_id}::{sha256(export_path)}::{plugin_name}"`. Deterministic — one document per file per plugin. |
| `type` | string | Always `"annotation"`. |
| `node_id` | string | ID of the node that owns the annotated file. |
| `export_path` | string | The file's `source.export_path`. |
| `plugin_name` | string | Name of the plugin that produced this annotation. Acts as the namespace — two plugins writing `summary` keys produce two separate annotation documents, not a collision. |
| `data` | object | The plugin's stdout JSON object, stored verbatim. Structure is defined entirely by the plugin. MosaicFS does not interpret or validate the contents. |
| `status` | string | `"ok"` or `"failed"`. Failed annotations are written when `max_attempts` is exhausted, preserving the failure record in the database. |
| `error` | string? | Present when `status` is `"failed"`. Last error message from the plugin invocation. |
| `annotated_at` | string | ISO 8601 timestamp when this annotation was last written. Compared against the file's `mtime` by the plugin runner to determine whether re-annotation is needed on a reconciliation crawl or full sync. |
| `updated_at` | string | ISO 8601 timestamp of last document modification. |

---

### Notification Document

A system event or condition requiring user attention. Written by agents, cloud bridges, the control plane, and plugins. Replicated to the browser via PouchDB for live delivery. The `_id` scheme deduplicates notifications so a recurring condition updates the existing document rather than accumulating duplicates.

| Field | Type | Description |
|---|---|---|
| `_id` | string | Format: `"notification::{source_id}::{condition_key}"`. `source_id` is the node ID, `"control_plane"`, or a bridge identifier. `condition_key` is a stable string identifying the condition type — e.g. `"oauth_expired"`, `"replication_lag"`, `"inotify_limit_approaching"`, `"plugin_jobs_failed:fulltext-search"`. Using a deterministic `_id` means a recurring condition performs an upsert rather than creating a new document. |
| `type` | string | Always `"notification"`. |
| `source.node_id` | string | ID of the node, bridge, or `"control_plane"` that produced this notification. Used to route the "View details" action and to filter notifications by source in the UI. |
| `source.component` | string | The subsystem that produced the notification: `"crawler"`, `"watcher"`, `"replication"`, `"cache"`, `"bridge"`, `"plugin:{plugin_name}"`, `"control_plane"`, etc. Displayed alongside the source node badge in the UI. |
| `severity` | string | `"info"`, `"warning"`, or `"error"`. Controls visual treatment in the UI and sort order in the notification panel. |
| `status` | string | `"active"`, `"resolved"`, or `"acknowledged"`. Writers set `"active"` when the condition arises and `"resolved"` when it clears. The user sets `"acknowledged"` via the UI or REST API. An acknowledged notification that transitions back to `"active"` (condition recurred) un-acknowledges automatically — the `status` field is set to `"active"` again by the writer. |
| `title` | string | Short notification title shown in the notification bell panel and dashboard alert area. e.g. `"OAuth token expired"`, `"Meilisearch index lag"`. |
| `message` | string | Full human-readable description of the condition. Shown in the notification detail view. May include counts, paths, or specific error messages. e.g. `"Google Drive OAuth token expired on 2026-02-14. Re-authorization is required to resume syncing."` |
| `actions` | array? | Optional list of actions the user can take directly from the notification. Each action has a `label` string and an `api` field containing a REST API path to call when the button is clicked. e.g. `{ "label": "Re-authorize", "api": "GET /api/nodes/{node_id}/auth" }`. The UI renders these as buttons in the notification detail view. |
| `condition_key` | string | The stable condition identifier, extracted from `_id` for convenience. Used by writers to check whether an existing notification document exists before deciding to create or update. |
| `first_seen` | string | ISO 8601 timestamp when this condition was first observed. Not updated on subsequent occurrences — preserves the original onset time. |
| `last_seen` | string | ISO 8601 timestamp of the most recent occurrence or update. Updated on every write. |
| `occurrence_count` | int | Number of times this condition has been written since `first_seen`. Incremented on each upsert. Displayed in the UI as "47 occurrences since Feb 14" for high-frequency conditions. |
| `acknowledged_at` | string? | ISO 8601 timestamp when the user acknowledged this notification. Cleared when the notification transitions back to `"active"`. |
| `resolved_at` | string? | ISO 8601 timestamp when the condition resolved. Present only when `status` is `"resolved"`. |

**Condition keys by source:**

| Source | Condition Key | Severity | Auto-resolves |
|---|---|---|---|
| Agent crawler | `first_crawl_complete` | info | No (one-shot, stays resolved) |
| Agent crawler | `inotify_limit_approaching` | warning | Yes (clears when watches freed) |
| Agent watcher | `watch_path_inaccessible:{path}` | error | Yes |
| Agent replication | `replication_lag` | warning | Yes |
| Agent cache | `cache_near_capacity` | warning | Yes |
| Cloud bridge | `oauth_expired:{bridge}` | error | Yes (clears on re-auth) |
| Cloud bridge | `oauth_expiring_soon:{bridge}` | warning | Yes |
| Cloud bridge | `quota_near_limit:{bridge}` | warning | Yes |
| Cloud bridge | `sync_stalled:{bridge}` | error | Yes |
| Cloud bridge | `large_remote_deletion:{bridge}` | warning | No (requires ack) |
| Plugin (executable) | `plugin_jobs_failed:{plugin_name}` | error | Yes (clears when queue drains) |
| Plugin (socket) | `plugin_disconnected:{plugin_name}` | warning | Yes (clears on reconnect) |
| Plugin (socket) | `plugin_health_check_failed:{plugin_name}` | warning | Yes |
| Plugin (any) | Arbitrary `condition_key` from health check response | Any | Plugin-controlled |
| Control plane | `new_node_registered:{node_id}` | info | No (requires ack) |
| Control plane | `credential_inactive:{key_id}` | warning | No (requires ack) |
| Control plane | `control_plane_disk_low` | warning | Yes |

**Plugin-issued notifications via health check:**

Socket plugins emit notifications in the health check response. The agent writes the notification document on the plugin's behalf — the plugin never writes to CouchDB directly. The plugin controls `condition_key`, `severity`, `title`, `message`, `actions`, and `resolve_notifications` (an array of `condition_key` values to mark resolved). The agent prefixes the `_id` with `notification::{node_id}::plugin:{plugin_name}:` to namespace plugin notifications under their source.

```json
{
  "status": "healthy",
  "notifications": [
    {
      "condition_key": "index_lag",
      "severity": "warning",
      "title": "Meilisearch index lag",
      "message": "Index is 1,847 documents behind. Last sync 4 minutes ago.",
      "actions": [
        { "label": "Trigger sync", "api": "POST /api/nodes/{node_id}/plugins/fulltext-search/sync" }
      ]
    }
  ],
  "resolve_notifications": ["index_lag_critical"]
}
``` Inode numbers are a concept native to FUSE; macOS File Provider and Windows CFAPI use analogous stable file identity tokens. The inode space is partitioned as follows:

| Range | Purpose |
|---|---|
| `0` | Reserved. FUSE treats 0 as invalid. |
| `1` | Root directory `"/"`. Stored in CouchDB as `_id "dir::root"`. |
| `2–999` | Reserved for future well-known synthetic entries. |
| `1000+` | Randomly assigned 64-bit integers, stored in the `inode` field of each document. |

The system is explicitly 64-bit only. A compile-time assertion in the Rust build script produces a human-readable error if compilation is attempted on a 32-bit platform.

---

## VFS Tiered Access Strategy

When the VFS layer needs to open a file, it evaluates access tiers in order of increasing cost, stopping at the first available option. This logic lives in the common `mosaicfs-vfs` crate and is shared across all OS-specific backends:

**Tier 1 — Local file.** The file lives on this node. Open directly via the real path.

**Tier 2 — Network mount (CIFS/NFS).** The owning node's document contains a `network_mounts` entry covering this file's export path. Translate and open via the local mount point recorded in that entry.

**Tier 3 — Local cloud sync directory.** The owning node's document contains a `network_mounts` entry of type `icloud_local` or `gdrive_local` covering this file. Open via the local sync directory, with eviction check for iCloud. If the file is evicted from local iCloud storage, fall through to Tier 4 rather than triggering an implicit cloud download.

**Tier 4 — Control plane bridge fetch.** No local access path is available. Request the file from the control plane's transfer endpoint. Stream to the path-keyed cache, verify the `Digest` trailer, serve from cache.

**Tier 5 — Plugin materialize.** The file's `export_path` matches the `file_path_prefix` of a `provides_filesystem` plugin on the owning node. The transfer server on the owning node invokes the plugin's `materialize` action, which writes the file to `cache/tmp/`. The agent moves it into the VFS cache and serves from there. Subsequent requests hit the cache directly without involving the plugin.

The full transfer server logic on the owning agent:

```
GET /api/agent/transfer/{file_id}
  → look up file document → get node_id, export_path, mtime, size
  → compute cache_key = SHA-256({node_id}::{export_path})
  → check cache/index.db:
      hit and mtime/size matches → serve from cache/{shard}/{cache_key}  ← fast path
      miss or stale:
        → check if export_path matches any plugin's file_path_prefix
        → if yes (Tier 5):
            staging_path = cache/tmp/plugin-{file_id}
            invoke plugin: materialize event with file_id, export_path, staging_path
            plugin writes bytes to staging_path, returns { size }
            move staging_path → cache/{shard}/{cache_key}  (atomic)
            insert/update row in cache/index.db
            serve from cache/{shard}/{cache_key}
        → if no (Tier 4 — remote file on another node):
            fetch via GET /api/agent/transfer/{file_id} from owning node
            stream to cache/tmp/{cache_key}
            verify Digest trailer
            move to cache/{shard}/{cache_key}
            insert row in cache/index.db
            serve from cache
```

The Digest trailer step is skipped for Tier 5 materialize — the plugin writes locally and there is no network transfer to verify. TLS is not involved. The agent trusts the plugin's output for the same reason it trusts any local process writing to the cache directory.

---

## Authentication

### Credential Format

Access keys follow the AWS naming convention. The access key ID is a public identifier safe to include in logs. The secret key is shown once at creation time and stored only as an Argon2id hash.

```
Access Key ID:  MOSAICFS_7F3A9B2C1D4E5F6A   (public)
Secret Key:     mosaicfs_<43 url-safe base64 chars>  (shown once)
```

### Agent-to-Server: HMAC Request Signing

Agents authenticate to the control plane using HMAC-SHA256 request signing. The signed string is a canonical concatenation of the HTTP method, path, ISO 8601 timestamp, and SHA-256 body hash. Requests with a timestamp older than 5 minutes are rejected to prevent replay attacks.

```
Authorization: MOSAICFS-HMAC-SHA256
  AccessKeyId=MOSAICFS_7F3A9B2C1D4E5F6A
  Timestamp=2025-11-14T09:22:00Z
  Signature=<hmac-sha256-hex>
```

### Web UI: JWT Sessions

The browser authenticates by presenting access key credentials to `POST /api/auth/login`. On success, the server issues a short-lived JWT (24-hour expiry) stored in memory — never in `localStorage`. All subsequent API requests include the JWT as a Bearer token.

### Agent-to-Agent: Credential Presentation

When one agent requests a file from another agent's transfer server, it presents its own access key ID and a HMAC-signed request. The receiving agent validates against its local credential store — which is kept current via CouchDB replication — so transfer authentication works even if the control plane is temporarily unreachable.

---

## REST API Reference

The control plane exposes a single REST API consumed by all clients — the web UI, CLI, desktop app, and agents. All endpoints are prefixed with `/api/`. Client-facing endpoints (web UI, CLI, desktop app) authenticate with a Bearer JWT obtained from `POST /api/auth/login`. Agent-internal endpoints under `/api/agent/` authenticate with HMAC request signing.

**Response conventions:**
- List responses: `{ "items": [...], "total": n, "offset": n, "limit": n }`
- Single resource responses: the document object directly
- Errors: `{ "error": { "code": "...", "message": "..." } }`
- Pagination: `?limit=` (default 100, max 500) and `?offset=` on all list endpoints
- No CouchDB internals (`_rev`, `_id` prefixes, CouchDB error codes) are exposed through the API

### Auth

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/auth/login` | Exchange access key credentials for a JWT. Request body: `{ access_key_id, secret_key }`. Returns `{ token, expires_at }`. |
| `POST` | `/api/auth/logout` | Invalidate the current JWT. |
| `GET` | `/api/auth/whoami` | Return the current credential's name, type, and last-seen timestamp. |

### Nodes

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/nodes` | List all nodes. Supports `?kind=physical\|cloud_bridge` filter. |
| `GET` | `/api/nodes/{node_id}` | Get full node document including embedded `network_mounts`. |
| `POST` | `/api/nodes` | Register a new node. Called by `mosaicfs-agent init`. Returns the new node ID. |
| `PATCH` | `/api/nodes/{node_id}` | Update `friendly_name` or `watch_paths`. |
| `DELETE` | `/api/nodes/{node_id}` | Deregister a node. Sets status to `"disabled"`; does not delete the document. |
| `GET` | `/api/nodes/{node_id}/status` | Return the node's current `agent_status` document. |
| `GET` | `/api/nodes/{node_id}/files` | List files owned by this node. Paginated. |
| `GET` | `/api/nodes/{node_id}/storage` | Return storage topology and latest utilization snapshot for this node. |
| `GET` | `/api/nodes/{node_id}/utilization` | Return utilization snapshot history. Supports `?days=30`. |
| `POST` | `/api/nodes/{node_id}/sync` | Trigger an immediate sync for cloud bridge nodes. Returns `405` for physical agent nodes. |
| `GET` | `/api/nodes/{node_id}/auth` | Return OAuth status for cloud bridge nodes. Returns `405` for physical agent nodes. |
| `DELETE` | `/api/nodes/{node_id}/auth` | Revoke stored OAuth tokens for a cloud bridge node. |
| `POST` | `/api/nodes/{node_id}/auth/callback` | OAuth redirect target. Receives the authorization code and exchanges it for tokens. |

### Node Network Mounts

Mounts are embedded in the node document. These endpoints update the `network_mounts` array on the node document via the control plane API.

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/nodes/{node_id}/mounts` | List all network mounts declared on this node. |
| `POST` | `/api/nodes/{node_id}/mounts` | Add a network mount. Request body: `{ remote_node_id, remote_base_export_path, local_mount_path, mount_type, priority }`. |
| `PATCH` | `/api/nodes/{node_id}/mounts/{mount_id}` | Update a mount entry. |
| `DELETE` | `/api/nodes/{node_id}/mounts/{mount_id}` | Remove a mount entry. |

### Files

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/files` | List files. Supports `?node_id=`, `?status=active\|deleted`, `?mime_type=`. Paginated. |
| `GET` | `/api/files/{file_id}` | Get file metadata document. |
| `GET` | `/api/files/{file_id}/content` | Download file bytes. Supports `Range` headers for partial content. Sets `Content-Disposition` for browser downloads. Full-file responses (HTTP 200) include a `Digest` trailer (RFC 9530, `sha-256`) computed as the bytes stream — clients may verify after receipt. Range responses (HTTP 206) do not include a `Digest` trailer. The control plane resolves the owning node and proxies bytes transparently — the client does not need to know which node holds the file. |
| `GET` | `/api/files/by-path?path=...` | Resolve a virtual path to its file document. Returns `404` if no file is mapped to that path. |

### Virtual Filesystem

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/vfs?path=...` | List the contents of a virtual directory. Evaluates the directory's mount sources on demand and returns matching files and subdirectories. |
| `GET` | `/api/vfs/tree?path=...&depth=n` | Recursive directory tree from a given path, up to `depth` levels deep (default 3, max 10). |
| `POST` | `/api/vfs/directories` | Create a new virtual directory. Request body: `{ virtual_path, name }`. Returns the created directory document. The directory is initially empty — add mount sources via `PATCH`. |
| `GET` | `/api/vfs/directories/{path}` | Get a virtual directory document including its full `mounts` array. |
| `PATCH` | `/api/vfs/directories/{path}` | Update a directory: rename, toggle `enforce_steps_on_children`, add/replace/remove mount entries. |
| `DELETE` | `/api/vfs/directories/{path}` | Delete a virtual directory. Returns `409` if the directory has children. Pass `?force=true` to cascade-delete all descendants. Cascade deletion removes all descendant virtual directory documents. `system: true` directories cannot be deleted. |
| `POST` | `/api/vfs/directories/{path}/preview` | Evaluate the directory's current mount sources against the file index and return matching files. The request body may contain a draft `mounts` array not yet saved — preview runs against the submitted configuration. Paginated. |

### Search

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/search?q=...` | Search files by filename. Supports substring and glob patterns. Returns `name`, `source.node_id`, `size`, `mtime` per result. Supports `?limit=` and `?offset=`. |
| `GET` | `/api/search?label=...` | Filter files by label. Returns all active files whose effective label set (direct assignments ∪ matching label rules) contains the specified label. Multiple `?label=` parameters are ANDed. Combinable with `?q=` for label + filename search. |
| `GET` | `/api/search?annotation[plugin/key]=value` | Filter files by annotation value. `plugin` is the plugin name and `key` is a dot-notation path into the annotation `data` object. `value` is an exact string match; prefix with `~` for regex match. Multiple `?annotation[...]` parameters are ANDed. Combinable with `?q=` and `?label=`. |

### Plugins

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/nodes/{node_id}/plugins` | List all plugin configurations for a node. |
| `POST` | `/api/nodes/{node_id}/plugins` | Create a plugin configuration. Request body: `{ plugin_name, plugin_type, enabled, name, subscribed_events, mime_globs, config, workers, timeout_s, max_attempts }`. |
| `GET` | `/api/nodes/{node_id}/plugins/{plugin_name}` | Get a plugin configuration document. |
| `PATCH` | `/api/nodes/{node_id}/plugins/{plugin_name}` | Update a plugin configuration. Changes take effect on the agent within one changes-feed poll cycle — no restart required. |
| `DELETE` | `/api/nodes/{node_id}/plugins/{plugin_name}` | Delete a plugin configuration. In-flight jobs complete; no new jobs are enqueued. |
| `POST` | `/api/nodes/{node_id}/plugins/{plugin_name}/sync` | Trigger a full sync for a specific plugin on a node. Enqueues `file.added` events for all active files whose annotation is stale or absent, bracketed by `sync.started` and `sync.completed`. |
| `POST` | `/api/nodes/{node_id}/sync` | Trigger a full sync for all enabled plugins on a node. |
| `GET` | `/api/nodes/{node_id}/plugins/{plugin_name}/jobs` | List recent plugin jobs — pending, in-flight, and failed. Supports `?status=` filter. Useful for diagnosing plugin failures from the web UI. |

### Query

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/query` | Fan out a query to all online nodes advertising the requested capability. Request body: `{ capability, query }`. Returns an array of result envelopes, one per responding plugin. Nodes that do not respond within the timeout are omitted. The browser does not need to know which nodes or plugins exist — it sends a capability name and receives labelled results. |

### Notifications

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/notifications` | List notifications. Supports `?status=active`, `?status=acknowledged`, `?status=resolved`, `?severity=error` filters. Default returns all active and unacknowledged notifications, sorted by severity then `last_seen`. |
| `POST` | `/api/notifications/{notification_id}/acknowledge` | Acknowledge a notification. Sets `status` to `"acknowledged"` and records `acknowledged_at`. |
| `POST` | `/api/notifications/acknowledge-all` | Acknowledge all currently active notifications. Accepts optional `?severity=` filter to acknowledge only notifications of a given severity. |
| `GET` | `/api/notifications/history` | Returns resolved and acknowledged notifications. Supports `?limit=` and `?since=` (ISO 8601). Useful for the notification history log view. |

### Annotations

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/annotations?node_id=...&path=...` | Get all annotation documents for a specific file (all plugins). Returns an object keyed by `plugin_name`. |
| `GET` | `/api/annotations?node_id=...&path=...&plugin=...` | Get the annotation document for a specific file and plugin. Returns `404` if no annotation exists. |
| `DELETE` | `/api/annotations?node_id=...&path=...` | Delete all annotation documents for a file. The next plugin sync or file event will cause the plugin to re-annotate. |
| `DELETE` | `/api/annotations?node_id=...&path=...&plugin=...` | Delete the annotation document for a specific file and plugin. |

### Labels

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/labels` | List all distinct label strings currently in use across all assignments and rules. Useful for autocomplete in the web UI. |
| `GET` | `/api/labels/assignments?node_id=...&path=...` | Get the `label_assignment` document for a specific file. Returns `404` if no labels are assigned. |
| `PUT` | `/api/labels/assignments` | Create or replace a label assignment. Request body: `{ node_id, export_path, labels }`. Overwrites any existing assignment for that file. |
| `DELETE` | `/api/labels/assignments?node_id=...&path=...` | Remove all labels from a specific file. |
| `GET` | `/api/labels/rules` | List all label rules. Supports `?node_id=` and `?enabled=` filters. |
| `POST` | `/api/labels/rules` | Create a label rule. Request body: `{ node_id, path_prefix, labels, name, enabled }`. |
| `PATCH` | `/api/labels/rules/{rule_id}` | Update a label rule: change labels, rename, enable/disable. |
| `DELETE` | `/api/labels/rules/{rule_id}` | Delete a label rule. Does not affect individual file assignments. |
| `GET` | `/api/labels/effective?node_id=...&path=...` | Compute and return the effective label set for a specific file — the union of its direct assignment and all matching rules. Useful for debugging label configuration. |

### Credentials

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/credentials` | List all credentials. Secret key hashes are never returned. |
| `GET` | `/api/credentials/{key_id}` | Get credential detail: name, type, enabled status, created_at, last_seen. |
| `POST` | `/api/credentials` | Create a new credential. Returns the secret key once in the response — it is not stored in recoverable form and cannot be retrieved again. |
| `PATCH` | `/api/credentials/{key_id}` | Update name or enabled status. |
| `DELETE` | `/api/credentials/{key_id}` | Revoke a credential. The agent or client using it will be rejected on its next request. |

### Storage

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/storage` | System-wide storage overview: capacity and utilization for all nodes and cloud bridges in a single response. |
| `GET` | `/api/storage/{node_id}/history` | Utilization snapshot history for a specific node. Supports `?days=30`. |

### System

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/health` | Overall system health: `healthy`, `degraded`, or `unhealthy`, with a summary of node statuses. |
| `GET` | `/api/health/nodes` | Per-node health summary for all nodes. |
| `GET` | `/api/system/info` | Instance metadata: version, setup date, total node count, total indexed file count. |
| `POST` | `/api/system/reindex` | Trigger a full reindex of all nodes. Sends a reindex command to all online agents. |
| `GET` | `/api/system/backup?type=minimal\|full` | Generate and download a backup. `minimal` includes only essential user-generated data (virtual directories, labels, annotations, credentials, plugin configurations). `full` includes the entire CouchDB database. Returns a JSON file in CouchDB `_bulk_docs` format, streamed as `Content-Disposition: attachment`. Filename: `mosaicfs-backup-{type}-{timestamp}.json`. |
| `POST` | `/api/system/restore` | Restore from a backup file. Requires an empty database — the endpoint checks the document count and rejects the restore if any documents exist. Request body is the JSON backup file. Validates that all documents have recognized `type` fields. For minimal backups, the restore process writes documents directly and merges `network_mounts` into existing node documents. For full backups, performs a bulk write of all documents. Returns a summary: document count restored, errors encountered. |
| `GET` | `/api/system/backup/status` | Check whether the database is empty (restorable). Returns `{ empty: true\|false, document_count: N }`. Used by the web UI to conditionally show the restore button. |
| `DELETE` | `/api/system/data` | **Developer mode only.** Delete all documents from CouchDB to enable restore into a non-empty database. Returns 403 Forbidden unless the control plane was started with `--developer-mode`. Requires a confirmation token in the request body: `{ "confirm": "DELETE_ALL_DATA" }`. Returns `{ deleted_count: N }` on success. This endpoint is intended for development and testing workflows where quickly cycling between backup/restore states is useful. It should never be enabled in production — the safer path is to destroy and recreate the Docker Compose stack. |

### Agent Internal

These endpoints are authenticated with HMAC request signing rather than JWT. They are not intended for use by human-operated clients. The `/api/agent/` prefix is not documented in the web UI or CLI help output.

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/agent/heartbeat` | Agent posts an updated node document (status, last_heartbeat). |
| `POST` | `/api/agent/files/bulk` | Bulk upsert file documents. Request body: `{ docs: [...] }`. Response includes per-document success or error status. Partial failure is accepted — successfully processed documents are committed even if others fail. |
| `POST` | `/api/agent/status` | Post an `agent_status` document. |
| `POST` | `/api/agent/utilization` | Post a `utilization_snapshot` document. |
| `GET` | `/api/agent/credentials` | Fetch the current enabled credential list for P2P transfer authentication. |
| `GET` | `/api/agent/transfer/{file_id}` | Fetch file bytes for a specific file. Used by remote agents requesting files via P2P transfer. Validates that the requesting credential is known and the file exists. Full-file responses include a `Digest` trailer (RFC 9530, `sha-256`); range responses do not. Supports `Range` headers. |
| `POST` | `/api/agent/query` | Deliver a query to this agent's plugin runner. Called by the control plane when routing a `POST /api/query` request to a node whose `capabilities` array contains the requested capability. Request body: `{ capability, query }`. The agent fans the request to all locally configured plugins with a matching `query_endpoints[].capability`, collects their stdout responses, and returns the merged array of result envelopes. Authenticated with HMAC — not reachable by the browser directly. |

---

## Backup and Restore

MosaicFS supports two backup strategies: minimal backups that capture only essential user-generated data, and full backups that preserve the entire database including operational history.

### Minimal Backup

A minimal backup contains only documents that cannot be automatically regenerated by agents or would be prohibitively expensive to regenerate. The guiding principle: "Would this data be lost forever if we didn't back it up?" Minimal backups are designed for the clean reinstall use case — wiping CouchDB entirely and starting fresh without losing user work.

**Included document types:**
- `virtual_directory` — user-created directory structure and mount configurations
- `label_assignment` — user-applied labels to individual files
- `label_rule` — user-defined label rules for path prefixes
- `annotation` — plugin-generated metadata. Technically regenerable by running a full plugin sync, but for large deployments this could take hours and incur API costs (AI summarization, OCR services). Safer to preserve.
- `credential` — access key hashes. Without these, every agent and client would need to be re-registered and re-authorized.
- `plugin` — plugin configurations including user-entered settings
- Partial `node` documents — specifically the `network_mounts` array from each node. The backup extraction process creates synthetic node documents containing only `_id`, `friendly_name`, and `network_mounts`. On restore, these are merged into the newly-registered node documents via `PATCH /api/nodes/{node_id}`.

**Excluded document types:**
- `file` — regenerated by the agent crawler in minutes on next startup
- `agent_status` — operational snapshot, regenerated on next heartbeat
- `utilization_snapshot` — historical trend data, nice to have but not essential
- `notification` — transient state; conditions recur if still active after restore

**Size:** For a deployment with 50,000 files, a minimal backup is typically under 10 MB — it contains a few hundred virtual directory and plugin documents, plus annotations. The file documents (which represent the bulk of the database) are excluded.

### Full Backup

A full backup contains every document in CouchDB: all document types, all history, soft-deleted files, old notifications, utilization snapshots. This is the "disaster recovery" or "migrate to a new control plane host" scenario. The backup is a complete point-in-time snapshot of the database.

The full backup is generated via `GET /mosaicfs/_all_docs?include_docs=true` and written as-is into the JSON file. The restore is a single `POST /mosaicfs/_bulk_docs` call after verifying the database is empty.

**Size:** Proportional to the total indexed file count. A deployment with 50,000 files might produce a 50–100 MB full backup.

### Backup Format

Both backup types use the same format: a JSON file containing an array of CouchDB documents, matching the `_bulk_docs` request format. This means the restore operation is a straightforward bulk write with no transformation required.

```json
{
  "docs": [
    { "_id": "dir::root", "type": "virtual_directory", ... },
    { "_id": "credential::...", "type": "credential", ... },
    { "_id": "plugin::...", "type": "plugin", ... }
  ]
}
```

**Filename convention:** `mosaicfs-backup-minimal-2026-02-16T09-22-00Z.json` and `mosaicfs-backup-full-2026-02-16T09-22-00Z.json`. The ISO 8601 timestamp makes it easy to identify when a backup was taken.

### Restore Process

Restore is only permitted into an empty database. The control plane checks the document count before accepting a restore request and returns an error if any documents exist. This prevents accidentally overwriting a live system.

**Restore steps:**

1. User uploads the backup JSON file via `POST /api/system/restore`
2. Control plane validates the file: checks that `docs` is an array, every document has a `type` field, and all types are recognized
3. For minimal backups: control plane performs a bulk write of all documents except partial node documents; for partial node documents, it extracts the `network_mounts` array and queues it for later merging
4. For full backups: control plane performs a bulk write of all documents
5. Control plane returns a summary: `{ restored_count: N, errors: [...] }`
6. Web UI prompts the user to restart all agents

**Post-restore reconciliation:**

After a minimal backup restore, agents reconnect and discover their node documents are incomplete (no `storage`, no `status`, no `last_heartbeat`). The agent regenerates these fields and performs a full crawl to rebuild the file document collection. The `network_mounts` array restored from the backup is already present in the node document, so the VFS layer's tiered access system works immediately.

For full backup restores, agents see their existing node documents and file documents and perform a standard reconciliation crawl — the mtime/size fast-path means unchanged files are skipped.

### What Is Not Backed Up

**Cloud bridge OAuth tokens** are stored as encrypted files in `bridges/` on the control plane host, not in CouchDB. They are not included in either backup type. After restore, the user must re-authorize each cloud bridge via the OAuth flow. This is a one-time per-bridge operation, far less disruptive than re-registering every agent or re-running every plugin.

**Plugin-managed external state** — data stored by socket plugins in external systems (Meilisearch indexes, vector databases, external APIs) is not backed up by MosaicFS. Plugins are responsible for their own backup strategies. After restore, a full plugin sync rebuilds external indexes from the file collection.

### Triggering Backups

Backups are on-demand via the REST API and the web UI. The Settings page provides a "Download backup" button with a choice: "Minimal (essential data only)" or "Full (complete database)". The control plane generates the JSON and streams it back as an attachment. The user saves the file wherever they want — local disk, cloud storage, version control.

Scheduled automatic backups are not implemented in v1 but are a natural future extension — a background job that writes backups to a configured destination (S3 bucket, local directory) on a schedule.

---

## Agent Crawl and Watch Strategy

### Initial and Periodic Crawl

The agent walks all configured watch paths and emits `file` documents. For each file, it stats the path and checks whether `(export_path, size, mtime)` matches the existing document — if so, the file is unchanged and the document is skipped. Changed and new files are written in batches of up to 200 documents using CouchDB's `_bulk_docs` endpoint. No content hashing is performed during the crawl — change detection relies entirely on `size` and `mtime`. Full crawls are scheduled nightly as a consistency safety net.

### Incremental Watching

After the initial crawl, the agent uses the OS native filesystem event API (`inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows) via the Rust `notify` crate. Events are debounced over a 500ms window per path to handle noisy editor saves. Rename events are correlated into a single path-update operation rather than a delete-and-create pair.

### inotify Watch Limit

Linux defaults to 8,192 inotify watches. Installations with large directory trees should raise this limit. The agent installation process sets `fs.inotify.max_user_watches = 524288` via `/etc/sysctl.d/`. Directories that cannot be watched due to limit exhaustion fall back to coverage by the nightly full crawl, and their paths are recorded in the agent's `watch_state` document.

### Reconciliation After Reconnect

When an agent reconnects after being offline, it runs an expedited full crawl of all watched paths before resuming normal watch mode. The mtime/size fast-path makes this much faster than a cold crawl. Reconnection is detected by monitoring the CouchDB replication connection state.

### Agent Main Loop

```
startup
  → load config
  → connect to local CouchDB
  → start CouchDB replication (bidirectional, continuous)
  → if first run:  full crawl → write notification (first_crawl_complete, info) on completion
                               then start watcher
  → if resuming:   reconciliation crawl → then start watcher
  → start transfer HTTP server
  → start heartbeat (update node document every 30s)
  → enumerate plugin directory → report available_plugins in agent_status
  → load plugin configurations from local CouchDB (type=plugin, node_id=this_node)
  → start plugin runner:
      for each enabled executable plugin: start worker pool (N workers)
      for each enabled socket plugin: connect to /run/mosaicfs/plugin-sockets/<n>.sock
          on connect: replay unacknowledged events from SQLite queue
                      resolve plugin_disconnected notification for this plugin
          on disconnect: write plugin_disconnected notification
                         retry with exponential backoff
  → watch changes feed for plugin document updates → reload plugin config live
  → run watcher event loop
      on file.added / file.modified: enqueue plugin jobs for subscribed plugins
      on file.deleted: delete annotation documents for this file, notify subscribed plugins
      on watch path becoming inaccessible: write watch_path_inaccessible notification
      on watch path restored: resolve watch_path_inaccessible notification
  → on schedule (nightly):    run full crawl as consistency check
  → on schedule (hourly):     collect storage inventory, write utilization_snapshot
                              check inotify watch count → write/resolve inotify_limit_approaching
                              check cache utilization → write/resolve cache_near_capacity
  → on schedule (per plugin health_check_interval_s):
                              send health_check to each enabled socket plugin
                              process notifications[] → write/update notification documents
                              process resolve_notifications[] → mark notification documents resolved
                              after 3 missed responses: write plugin_health_check_failed notification
  → on sync request (manual or API): run full sync (see Plugin Full Sync below)
```

---

## Rule Evaluation Engine

### Evaluation Model

Rules are embedded in virtual directory documents as `mounts` arrays rather than existing as separate documents. Evaluation is on-demand: when the VFS layer receives a `readdir` call for a virtual directory, the rule engine evaluates that directory's mount sources at that moment against the file documents in the local CouchDB replica. There is no background worker and no pre-computed `virtual_path` on file documents — a file's virtual location is derived when someone asks for it.

This model has several attractive properties:

- **Changes take effect immediately.** Editing a directory's mounts is reflected on the next `readdir` — no background recomputation, no propagation delay.
- **Lazy evaluation.** A directory that is never accessed never evaluates its mounts. Deep trees with many directories are traversed only as the user navigates into them.
- **Multiple appearances are natural.** The same file can satisfy the mount criteria of many directories simultaneously. The engine evaluates each directory independently; there is no global deduplication or single-winner policy across the tree.
- **No orphaned virtual paths.** Because virtual paths are never stored, there is nothing to go stale when mounts change or files are deleted.

### Readdir Evaluation

When `readdir` is called for a virtual directory:

```
readdir(virtual_path)
  → load virtual_directory document from local replica
  → collect ancestor step chain (root → ... → parent)
      for each ancestor with enforce_steps_on_children: true
          prepend its mount steps to the inherited chain
  → for each mount in directory.mounts:
      query file documents matching source (node_id, export_path prefix)
      for each candidate file:
          if status != "active": skip
          resolve effective_labels(file)              ← join label_assignment + label_rules
          resolve annotations(file)                   ← load annotation docs for this file (lazy, per plugin_name referenced in steps)
          run inherited step chain → if excluded: skip
          run mount's own steps  → if excluded: skip
          apply mapping strategy to derive filename within this directory
          if name collision: apply conflict_policy
          emit (filename, file_doc)
  → also include child virtual_directory documents as subdirectory entries
  → return combined listing
```

The query for candidate files uses the `(type, source.node_id, source.export_parent)` index — fetching all files whose `export_parent` starts with the mount's `source.export_path`. This is an indexed prefix scan, not a full collection scan.

### Step Pipeline

Each mount's `steps` array is an ordered list of filter steps evaluated in sequence after any inherited ancestor steps. The eight supported ops are:

- **`glob`** — matches against the file's `export_path` using a glob pattern. Supports wildcards (`*`, `**`, `?`).
- **`regex`** — matches against the file's `export_path` using a regular expression. Supports a `flags` field (e.g. `"i"` for case-insensitive).
- **`age`** — compares `mtime`. `max_days` requires the file to be newer than N days; `min_days` requires it to be older. Both may be specified for a date range.
- **`size`** — compares file size in bytes. Accepts `min_bytes`, `max_bytes`, or both.
- **`mime`** — matches against `mime_type`. Accepts an array of type strings with wildcard support (e.g. `"image/*"`).
- **`node`** — matches files originating from a specific set of nodes by `node_ids` array.
- **`label`** — matches against the file's effective label set. Accepts a `labels` array; the step matches if the file's effective label set contains **all** of the specified labels (AND semantics). To implement OR semantics, use multiple label steps with `on_match: "include"`.
- **`annotation`** — matches against a value in a file's annotation data. Requires `plugin_name` (identifies which annotation document to inspect) and `key` (a dot-notation path into the `data` object, e.g. `"language"` or `"tags.primary"`). Optionally accepts `value` for exact match or `regex` for pattern match against a string value. If only `plugin_name` and `key` are provided, the step matches if the key exists with any non-null value. If the annotation document does not exist (the plugin has not yet processed the file), the step does not match.

Step evaluation logic:

```
for each step in [inherited_steps..., mount.steps...]:
    raw_match   = evaluate(step.op, file)
    final_match = step.invert ? !raw_match : raw_match
    if final_match:
        if step.on_match == "continue": proceed to next step
        if step.on_match == "include":  file → included (stop)
        if step.on_match == "exclude":  file → excluded (stop)
    else:
        proceed to next step

file → mount.default_result
```

A non-match always continues to the next step. Unknown `op` values are treated as a non-match and continue, allowing future op types to be introduced without breaking existing rule engine versions.

### Inheritance

When an ancestor directory has `enforce_steps_on_children: true`, its mount steps are prepended to every mount evaluation in all descendant directories — outermost ancestor first, nearest parent last. A child's own mount steps are appended last. Ancestor steps always evaluate before descendant steps. A child can narrow an ancestor's results further (by adding more restrictive steps) but cannot widen them (cannot surface files the ancestor excluded).

### Mapping Strategies

- **`prefix_replace`** — strip `source_prefix` from `export_path` and mount the remaining path hierarchy as a subtree within this directory. The most common case.
- **`flatten`** — place all matching files directly in this directory, discarding their subdirectory structure. All matching files appear as immediate children regardless of nesting depth.

### Conflict Resolution

When two mount sources within the same directory produce a file with the same name, the `conflict_policy` on the originating mount determines the outcome. `"last_write_wins"` keeps the file with the most recent `mtime`. `"suffix_node_id"` appends the node ID to the losing file's name, making both visible.

### Readdir Cache

The VFS layer caches the output of `readdir` for each virtual directory with a short TTL (default 5 seconds). This prevents re-evaluating mount sources on every `lookup` within a directory during rapid directory traversal. The cache is invalidated when the directory document changes via the PouchDB live changes feed — so edits to a directory's mounts take effect within one TTL window on agents that have already cached the listing, and immediately on the next `readdir` on agents that have not.

---

## Plugin System

### Plugin Runner Architecture

The plugin runner is a subsystem of the agent responsible for delivering file lifecycle events to configured plugins and writing annotation results back to CouchDB. It operates independently of the crawl and watch pipeline — the crawler and watcher enqueue events into a SQLite job table; the plugin runner drains that queue asynchronously.

```
/var/lib/mosaicfs/plugin_jobs.db   ← SQLite job queue (separate from cache index.db)

  table: jobs
    id           INTEGER PRIMARY KEY
    node_id      TEXT
    export_path  TEXT
    plugin_name  TEXT
    event_type   TEXT        -- file.added | file.modified | file.deleted | sync.started | sync.completed | crawl_requested | materialize
    payload_json TEXT        -- serialised event payload
    sequence     INTEGER     -- monotonically increasing per plugin
    status       TEXT        -- pending | in_flight | acked | failed
    attempts     INTEGER DEFAULT 0
    next_attempt TEXT        -- ISO 8601, for backoff scheduling
    created_at   TEXT
```

For executable plugins, workers poll the `pending` queue, set status to `in_flight`, invoke the binary, and on success write the annotation document and mark the job `acked`. On failure they increment `attempts` and set `next_attempt` with exponential backoff. After `max_attempts`, the job moves to `failed` and is surfaced in `agent_status`.

For socket plugins, the runner maintains one connection per plugin. Events are written to the socket in sequence order as soon as the socket is connected. When an ack arrives, the corresponding job row is updated to `acked`. On disconnect, all `in_flight` rows are reset to `pending` for replay on reconnect.

### Plugin Full Sync

A full sync replays all active files in the local CouchDB replica through the plugin pipeline. It is triggered by a manual user action from the web UI (per-plugin or for all plugins on the node) or by the `POST /api/nodes/{node_id}/plugins/{plugin_name}/sync` API endpoint.

```
full_sync(plugin_name)
  → emit sync.started event to plugin
  → query all file documents where status = "active" from local CouchDB
  → for each file:
      fetch annotation document for (file, plugin_name) if it exists
      if annotation.annotated_at >= file.mtime: skip  ← already current
      enqueue file.added job for this file
  → emit sync.completed event to plugin
```

This design means the full sync is idempotent — running it multiple times is safe. Files whose annotations are already current are skipped. Newly installed plugins have no annotation documents, so every file is enqueued. A plugin that crashed mid-sync will have a mix of annotated and unannotated files; the sync skips the ones already processed.

The `sync.started` and `sync.completed` events allow socket plugins to optimize their handling. A search indexer might suppress incremental commits during the sync window and perform one bulk commit on `sync.completed`, which is significantly faster than committing after every file.

### Available Plugins Discovery

On startup and whenever the plugin directory changes, the agent enumerates the platform-specific plugin directory and records the list of executable filenames in `agent_status.available_plugins`. The web UI reads this list when a user creates a new plugin configuration, populating the plugin name dropdown with only the binaries that are actually installed on that node. Attempting to create a plugin configuration for a name not in `available_plugins` is permitted (the binary could be installed later) but is flagged with a warning in the UI.

### Capability Advertisement

When a plugin comes online and has `query_endpoints` declared, the agent adds the corresponding capability strings to the node document's `capabilities` array. When a plugin goes offline (socket disconnects, binary missing, max_attempts exhausted), its capabilities are removed. The node document update is a standard CouchDB write that replicates to the control plane and browser within seconds.

The control plane maintains a live view of `capabilities` across all nodes by watching the CouchDB changes feed. When a `POST /api/query` request arrives, the control plane fans it out to all nodes whose `capabilities` array contains the requested capability. This means adding a new query capability to the system requires only: deploying a plugin binary with `query_endpoints` declared in its plugin document. No control plane code changes, no UI code changes.

```
capability advertisement lifecycle:

plugin socket connects
  → agent adds capability to node.capabilities
  → node document written to CouchDB
  → replicates to control plane within seconds
  → control plane begins routing queries to this node

plugin socket disconnects
  → agent removes capability from node.capabilities
  → control plane stops routing queries to this node
  → in-flight queries to this node time out gracefully
```

### Plugin Query Routing

The control plane exposes `POST /api/query` as the single query entry point for the browser. The request body specifies a `capability` and a `query` string. The control plane fans the request out to all online nodes advertising that capability, collects responses, and returns them as an array.

```
POST /api/query
  { "capability": "search", "query": "quarterly earnings" }

  → control plane finds all nodes where capabilities ∋ "search"
  → for each matching node:
      POST node.transfer.endpoint/query
           { "query": "quarterly earnings", "capability": "search" }
      collect response or record timeout
  → return array of result envelopes to browser:
  [
    {
      "node_id":     "plugin-agent-01",
      "plugin_name": "fulltext-search",
      "capability":  "search",
      "description": "Full-text search powered by Meilisearch",
      "results":     [ ... ]
    },
    {
      "node_id":     "plugin-agent-01",
      "plugin_name": "semantic-search",
      "capability":  "search",
      "description": "Semantic similarity search",
      "results":     [ ... ]
    }
  ]
```

Nodes that do not respond within a configurable timeout (default 5 seconds) are omitted from the results — a slow or unavailable plugin degrades gracefully rather than blocking the entire query. The browser renders each result envelope as a separate labelled section, identified by `description`.

**Plugin-agent node in Docker Compose**

A plugin-agent is a standard MosaicFS agent running in the Docker Compose stack alongside the control plane, with no watch paths and no VFS mount. It registers as a normal `node_kind: "physical"` node and is indistinguishable from a regular agent in the data model. The only difference is operational: it runs in a container with access to the Compose internal network, allowing plugin binaries to reach services like Meilisearch by hostname.

```yaml
services:
  control-plane:
    image: mosaicfs-server

  meilisearch:
    image: getmeili/meilisearch
    volumes:
      - meilisearch_data:/meili_data

  plugin-agent:
    image: mosaicfs-agent
    environment:
      - MOSAICFS_SERVER_URL=http://control-plane:8080
      - MOSAICFS_SECRET_KEY=...
    volumes:
      - ./plugins:/usr/lib/mosaicfs/plugins
```

The `fulltext-search` plugin binary in `/usr/lib/mosaicfs/plugins/` connects to `meilisearch:7700` directly over the Compose network. No external networking, no additional credentials. The indexing plugin (agent side, running on each physical node) and the query plugin (running on the plugin-agent node) are two separate binaries that share the same Meilisearch instance — the indexing plugin writes, the query plugin reads.

### Bridge Nodes

A bridge node is a generalized data source adapter — a MosaicFS agent with no real filesystem watch paths, a dedicated bridge storage volume, and one or more `provides_filesystem` plugins that act as its filesystem implementation. Bridge nodes extend the plugin model to cover not just file processing and annotation, but file *creation* from external data sources.

**Relationship to the existing agent model**

A bridge node runs the standard `mosaicfs-agent` binary. It registers as `node_kind: "physical"` and participates in the same document model, replication flows, and health monitoring as any other agent. The only operational differences are:

- `agent.toml` declares no `watch_paths` — the agent does not crawl a real filesystem
- One or more plugins have `provides_filesystem: true` and a unique `file_path_prefix`
- The node has bridge storage (a Docker volume) rather than user filesystems
- The agent invokes `crawl_requested` events on its plugins instead of walking directories

**Bridge storage**

Bridge storage is a Docker volume mounted into the bridge node container at a well-known path. It is divided into two directories:

```
/var/lib/mosaicfs/bridge-data/
  files/              ← the export tree — paths appearing in file document export_paths
    gmail/
      2026/02/
        re-project-kickoff.eml
  plugin-state/       ← plugin-managed internal state, not exposed as MosaicFS files
    email-fetch/
      messages.db     ← SQLite: raw message storage for Option 2 aggregate storage
      sync-state.json ← last sync cursor, OAuth tokens, etc.
    fulltext-search/
      index.db        ← search index maintained by the indexing plugin
```

`files/` contains real files on disk — the content that appears in the virtual filesystem. Plugins write to `files/` during crawl and delete from `files/` when data is retired. The agent serves these files directly via Tier 1 (local file) since the bridge node owns them. No Tier 5 materialize invocation is needed for files that exist on disk in `files/`.

`plugin-state/` is opaque to MosaicFS. Plugins organize it however they need. It is included in full backups (it is part of the Docker volume) but excluded from minimal backups (it is regenerable by re-running the plugin).

**Two storage strategies for plugin-owned files**

Bridge plugins can choose how they store data internally:

*Option A — One file per record (files/ approach).* The plugin writes one `.eml` per email, one `.ics` per calendar event, organized under `files/` with date-based sharding to avoid directory size problems. The agent serves them via Tier 1. No materialize invocation needed. Simple, debuggable, compatible with standard file tools.

```
files/gmail/2026/02/16/re-project-kickoff.eml   ← real file on disk
files/gmail/2026/02/16/status-update.eml
```

Volume format: `mkfs.ext4 -N 2000000` to provision sufficient inodes for large email archives. The agent monitors inode utilization on bridge storage and writes an `inodes_near_exhaustion` notification when approaching the limit.

*Option B — Aggregate storage with Tier 5 materialize.* The plugin stores all records in `plugin-state/` (e.g., a SQLite database of message bodies) and registers files in CouchDB with export paths under `files/`. Files do not exist on disk until first access, at which point the transfer server invokes the `materialize` action and the agent writes the result into the VFS cache. Subsequent accesses hit the cache.

```
plugin-state/email-fetch/messages.db   ← SQLite with all message bodies
                                          (no individual .eml files on disk)
```

Option A is recommended for v1. Option B is the upgrade path if inode exhaustion becomes a problem in practice — the `provides_filesystem` and `file_path_prefix` fields are already in the schema, and the Tier 5 materialize path is already implemented, so the switch requires only a plugin implementation change with no agent or schema updates.

**Bridge node in Docker Compose**

```yaml
services:
  control-plane:
    image: mosaicfs-server

  bridge-email:
    image: mosaicfs-agent
    environment:
      - MOSAICFS_SERVER_URL=http://control-plane:8080
      - MOSAICFS_SECRET_KEY=...
      - MOSAICFS_NODE_NAME=Email Bridge
    volumes:
      - ./plugins:/usr/lib/mosaicfs/plugins
      - email-bridge-data:/var/lib/mosaicfs/bridge-data

volumes:
  email-bridge-data:
    driver: local
    driver_opts:
      type: none
      o: bind
      device: /mnt/data/mosaicfs-email-bridge
```

The `agent.toml` for a bridge node:

```toml
[agent]
watch_paths = []   # no real filesystem to watch

[bridge]
bridge_data_path = "/var/lib/mosaicfs/bridge-data"
```

**Node document for a bridge node**

Bridge nodes self-identify via a `role` field added to the node document. The web UI uses this to render the node detail page appropriately — showing "Bridge Storage" instead of "Storage Topology" and surfacing retention configuration rather than volume/disk topology.

```json
{
  "_id":           "node::bridge-email-01",
  "type":          "node",
  "node_kind":     "physical",
  "role":          "bridge",
  "friendly_name": "Email Bridge",
  "platform":      "linux",
  "vfs_capable":   false,
  "storage": [
    {
      "filesystem_id":       "email-bridge-data",
      "mount_point":         "/var/lib/mosaicfs/bridge-data",
      "fs_type":             "ext4",
      "capacity_bytes":      107374182400,
      "used_bytes":          21474836480,
      "watch_paths_on_fs":   []
    }
  ]
}
```

The `role: "bridge"` field is optional and defaults to `null` on physical nodes. Bridge nodes set it to `"bridge"`. The UI checks this field to conditionally show bridge-specific controls; all other agent logic treats bridge nodes identically to physical nodes.

**Agent main loop for bridge nodes**

The crawl step is replaced by plugin invocation:

```
startup (bridge node)
  → load config
  → connect to local CouchDB
  → start CouchDB replication (bidirectional, continuous)
  → load plugin configurations → find plugins where provides_filesystem = true
  → for each filesystem-providing plugin:
      emit crawl_requested (trigger: "startup") → apply file operations to CouchDB
  → start transfer HTTP server  (serves files from bridge-data/files/ via Tier 1)
  → start heartbeat
  → start plugin runner (for non-filesystem plugins: annotators, indexers, etc.)
  → on schedule (nightly): emit crawl_requested (trigger: "scheduled") to each filesystem plugin
  → on schedule (hourly):  collect bridge storage utilization → write utilization_snapshot
                           check inode utilization → write/resolve inodes_near_exhaustion
                           check storage utilization → write/resolve storage_near_capacity
  → on manual sync request: emit crawl_requested (trigger: "manual")
```

**Bridge storage monitoring**

The agent's hourly utilization check covers both disk space and inode utilization on bridge storage volumes. Two additional notification condition keys are defined for bridge nodes:

| Condition Key | Severity | Auto-resolves |
|---|---|---|
| `inodes_near_exhaustion` | warning | Yes (clears when inodes freed) |
| `storage_near_capacity` | warning | Yes (clears when space freed) |

**Plugin document example for an email bridge plugin**

```json
{
  "_id":                "plugin::bridge-email-01::email-fetch",
  "type":               "plugin",
  "node_id":            "bridge-email-01",
  "plugin_name":        "email-fetch",
  "plugin_type":        "executable",
  "enabled":            true,
  "name":               "Gmail Fetcher",
  "subscribed_events":  ["crawl_requested"],
  "provides_filesystem": true,
  "file_path_prefix":   "/gmail",
  "settings_schema": {
    "properties": {
      "gmail_client_id":     { "type": "string", "title": "Gmail Client ID" },
      "gmail_client_secret": { "type": "secret", "title": "Gmail Client Secret" },
      "fetch_all":           { "type": "boolean", "title": "Fetch all mail", "default": true },
      "fetch_days":          { "type": "number",  "title": "Fetch last N days (if not all)", "default": 30 },
      "auto_delete_days":    { "type": "number",  "title": "Auto-delete older than N days (0 = never)", "default": 0 }
    },
    "required": ["gmail_client_id", "gmail_client_secret"]
  },
  "settings": {
    "gmail_client_id": "...",
    "gmail_client_secret": "...",
    "fetch_all": true,
    "auto_delete_days": 0
  },
  "created_at": "2026-02-16T09:00:00Z"
}
```

Other plugins on the same bridge node (annotators, search indexers) are configured identically to plugins on physical nodes — they subscribe to `file.added`, `file.modified`, and `file.deleted` events that the agent emits as the email-fetch plugin's crawl responses are applied to CouchDB.

The plugin directory is the security boundary. An entry in a `plugin` CouchDB document cannot cause the agent to execute a binary outside the plugin directory, regardless of what `plugin_name` contains. The agent resolves `plugin_name` by joining it to the plugin directory path and checking that the resulting path is a regular, executable file — path traversal characters (`.`, `/`) in `plugin_name` are rejected as a permanent error. A malicious `plugin_name` value of `../../bin/sh` fails immediately rather than executing a shell.

The `config` object in the plugin document is passed to the plugin on stdin as part of the event envelope. It is treated as data, not as shell arguments — there is no shell interpolation at any point in the invocation path. The agent invokes the binary directly via `execv`, not via a shell.

### Future Directions

The Unix socket model extends naturally to networked plugins: replacing the socket path with a TCP address would allow plugins to run on a different machine from the agent. The event envelope and ack protocol are identical. This is not implemented in v1 but the document schema and protocol are designed to accommodate it — a future `plugin_type: "tcp"` variant would add a `socket_address` field alongside the existing `plugin_name`.

---

## Search

### v1: Filename Search

In v1, search operates entirely on data already present in CouchDB — no additional indexing infrastructure is required. The search API endpoint accepts a query string and returns matching file documents. Two match modes are supported:

**Substring match** — returns all files where the `name` field contains the query string, case-insensitively. Suitable for quick lookups by partial filename.

**Glob match** — interprets the query as a glob pattern (`*.pdf`, `report-2025-*`, `**/*.rs`) and matches against `name`. Evaluated on the control plane against the CouchDB result set.

The search endpoint is:

```
GET /api/search?q=<query>&limit=100&offset=0
```

Results include `name`, `source.node_id`, `size`, and `mtime` for each match. Virtual paths are not included — a file may appear in multiple virtual directories and has no single canonical virtual path. The web UI search page and the CLI `mosaicfs-cli files search` command are both thin wrappers around this endpoint.

Because search is backed by CouchDB's existing file documents, it reflects the current state of the index in real time — new files appear in search results as soon as the agent indexes them and the document replicates to the control plane.

### Future: Richer Search

Filename search covers the most common retrieval need but leaves several useful capabilities for future versions:

**Metadata filtering** — constraining results by file type (MIME type or extension), size range, modification date range, or owning node. This requires no new infrastructure, only additional query parameters on the existing search endpoint and corresponding CouchDB Mango query expressions.

**Full-text content search** — searching inside the contents of documents, PDFs, source code, and other text-bearing file formats. This is a substantially larger undertaking: it requires fetching files from their owning nodes, running format-specific text extraction, and maintaining a dedicated search index. Tantivy (a Rust-native full-text search library) or Meilisearch are the candidate engines. Content search is out of scope for v1 but is the most significant planned search enhancement.

---

## VFS File Cache

The cache is part of the common `mosaicfs-vfs` layer, shared across all OS-specific backends. It has two operating modes that share a common on-disk layout and SQLite index. **Full-file mode** is used for small files — the entire file is downloaded in one request and cached atomically. **Block mode** is used for large files, primarily home media (videos, audio, large archives) — individual regions of the file are fetched on demand as read requests arrive, allowing a 40 GB video to be played without downloading it in full. The threshold between modes is configurable (default 50 MB).

### On-Disk Structure

```
/var/lib/MosaicFS/cache/
  a3/f72b1c...          # sparse data file, keyed by SHA-256({node_id}::{export_path})
  tmp/                  # in-progress full-file downloads
  index.db              # SQLite: all cache metadata
```

Each cache entry is keyed by a SHA-256 hash of the string `{node_id}::{export_path}` — deterministic and derived from file identity, not file contents. The first two characters of this key are used as a shard prefix. All data files are sparse files: the OS allocates disk space only for regions that have been written, and unwritten regions read as zeros without consuming disk space. Sparse file support is available on ext4, APFS, NTFS, and ZFS — all target platforms for MosaicFS agents.

### SQLite Index Schema

The `index.db` SQLite database is the single source of truth for all cache metadata. It is updated transactionally so a crash between a write and a metadata update cannot produce an inconsistent state.

```sql
CREATE TABLE cache_entries (
    cache_key       TEXT PRIMARY KEY,   -- SHA-256({node_id}::{export_path})
    node_id         TEXT NOT NULL,
    export_path     TEXT NOT NULL,
    file_size       INTEGER NOT NULL,
    mtime           TEXT NOT NULL,      -- ISO 8601, for invalidation
    size_on_record  INTEGER NOT NULL,   -- file size at cache time, for invalidation
    block_size      INTEGER,            -- NULL for full-file entries
    block_map       BLOB,               -- NULL for full-file entries; see Block Map below
    cached_bytes    INTEGER NOT NULL,   -- bytes present on disk, for eviction accounting
    last_access     TEXT NOT NULL,      -- ISO 8601, for LRU eviction
    source          TEXT NOT NULL DEFAULT 'remote'
                                        -- 'remote' | 'plugin:{plugin_name}'
);
```

The `source` column is diagnostic — it identifies whether a cache entry was populated by a Tier 4 remote download or by a Tier 5 plugin materialize. It has no effect on cache logic; it is reported in `agent_status` cache statistics and useful for debugging bridge node behavior.

### Block Map

For block-mode entries, the `block_map` column stores a sorted list of non-overlapping, non-adjacent intervals of present blocks. Each interval is a `[start_block, end_block)` half-open range in block-index units.

```
Example: 40 GB file at 1 MB block size
  User watched 0:00–5:00, then seeked to 45:00 and watched 45:00–50:00

  block_map = [(0, 300), (2700, 3000)]
              first 300 blocks present, blocks 2700–2999 present
              2 intervals, ~40 bytes serialized
```

This representation is efficient for the home media use case. A typical viewing session with a few seeks produces 3–15 intervals regardless of file size. The intervals are stored as a compact binary blob (pairs of little-endian u64 values) in the `block_map` column.

**Operations on the block map:**

*Is block N present?* Binary search the interval list for an interval containing N. O(log k) where k is the number of intervals — in practice k < 20.

*Which blocks are missing in range [A, B)?* Take the requested range, subtract all overlapping intervals, and return the remaining gaps as a list of sub-ranges. Each gap becomes one HTTP range request to the remote agent.

*Mark blocks [A, B) as present:* Insert the interval, then merge with any adjacent or overlapping intervals to maintain the sorted, non-overlapping invariant. O(k) worst case, O(1) for the common case of appending to the last interval (sequential playback).

A roaring bitmap is a suitable future upgrade if access patterns turn out to be more fragmented than the home media use case produces — for example, random-access reads against large database files. For now the interval list is simpler, debuggable, and amply fast.

### Full-File Mode Request Flow

```
read(file_id, offset, length) — file below threshold

  cache hit:
    read bytes from sparse data file at offset, return

  cache miss:
    fetch full file via GET /api/agent/transfer/{file_id}  (HTTP 200)
    stream to cache/tmp/{cache_key}
    verify Digest trailer
    move atomically to cache/{shard}/{cache_key}
    insert row into index.db (block_map = NULL)
    return requested bytes
```

### Block Mode Request Flow

```
read(file_id, offset, length) — file above threshold

  compute block range:
    first_block = offset / block_size
    last_block  = (offset + length - 1) / block_size

  check block map for blocks [first_block, last_block]:
    all present → read bytes from sparse data file, return immediately

    some missing → compute list of missing sub-ranges
                   coalesce adjacent missing blocks into contiguous spans
                   for each span:
                     fetch via GET /api/agent/transfer/{file_id}
                                  Range: bytes={start}-{end}  (HTTP 206)
                     write fetched bytes into sparse data file at correct offset
                   update block map in index.db (single transaction)
                   read assembled bytes from sparse data file, return
```

Missing blocks are coalesced before fetching: if blocks 5, 6, and 7 are all absent, one `Range: bytes=5242880-8388607` request is issued rather than three separate requests. This minimises round trips during the common case of sequential playback with a cold cache.

### Transfer Integrity

Full-file responses (HTTP 200) include a `Digest` trailer (RFC 9530, `sha-256`) computed as the bytes stream. The receiving agent accumulates the hash as it writes to `cache/tmp/`, reads the trailer after the stream closes, and verifies before moving the file to its final location. A mismatch causes the download to be discarded and retried.

```
Trailer: Digest
[response body stream]
Digest: sha-256=:base64encodedchecksum:
```

Range responses (HTTP 206) used by block mode do not carry a `Digest` trailer. TLS provides in-transit integrity for partial fetches. This is appropriate for the threat model: MosaicFS agents are trusted nodes on a home network, and the primary integrity risk is bugs in streaming code (truncation, wrong offset) rather than a malicious intermediary. A corrupted block produces visible video artefacts rather than silent data loss, which is detectable and recoverable by re-fetching the affected blocks.

### Eviction

LRU eviction operates on whole cache entries, not individual blocks. An entry's `cached_bytes` column tracks how much disk space it occupies (for block-mode entries, the number of present blocks × block size). The evictor selects entries by ascending `last_access`, accumulating `cached_bytes` until the size cap or free space constraint is satisfied.

Configurable thresholds: total cache size cap (default 10 GB) and minimum free space on the cache volume (default 1 GB). Eviction runs as a background task after each new cache write.

Partially-cached block-mode entries are evictable — the entire sparse file and its `index.db` row are removed together. There is no partial eviction of individual blocks within an entry. This keeps the eviction logic simple at the cost of occasionally re-fetching blocks that were recently accessed but belonged to an entry that was evicted as a whole.

### Invalidation

The PouchDB live changes feed triggers invalidation when a file document's `mtime` or `size` changes. On each cache lookup, the stored `mtime` and `size_on_record` are compared to the current file document. A mismatch causes the entry to be treated as stale: the sparse data file and `index.db` row are deleted, and the file is re-fetched on the next read.

On agent startup, the cache index is reconciled against the local CouchDB replica. Entries with no matching file document are removed. Entries whose `mtime` or `size` no longer matches the document are removed.

### Download Deduplication

Concurrent reads for the same uncached block range share a single in-flight fetch via a `Shared` future keyed by `(file_id, block_range)`. All waiters receive the result when the fetch completes. This prevents redundant range requests when multiple processes read the same region of a file simultaneously — common when a video player issues read-ahead requests in parallel.

---

## Deployment

### Control Plane

The control plane runs as a Docker Compose stack. CouchDB is bound to localhost only and is not directly reachable from outside the host. The Axum API server is the only externally-accessible process — it serves the REST API, proxies the CouchDB replication endpoint for authenticated agent connections, issues PouchDB session tokens for browser clients, and terminates TLS. On first start, the Compose stack initialises CouchDB with an admin credential and creates the `mosaicfs_browser` CouchDB role with read-only access to the `mosaicfs` database. TLS is enabled by default using an automatically generated self-signed CA and server certificate.

**Developer mode:** The control plane binary accepts a `--developer-mode` flag. When enabled, the `DELETE /api/system/data` endpoint becomes accessible, allowing complete database wipes without destroying the Docker Compose stack. This is intended for development and testing workflows where quickly cycling between backup/restore states is useful. Developer mode should never be enabled in production — a production database wipe should be done by destroying and recreating the Compose stack, not via an API endpoint. The flag is disabled by default.

### Agents

Agents are distributed as single static binaries. The `MosaicFS-agent init` command configures the agent, registers it with the control plane, installs the systemd unit (or launchd plist on macOS), and starts the service. The secret key is never passed as a CLI argument; it is read from stdin with echo disabled via the `rpassword` crate, or from the `MOSAICFS_SECRET_KEY` environment variable for scripted deployments.

### State Directory

```
/var/lib/mosaicfs/
  agent.toml          # configuration
  node_id             # persistent node identity
  pouchdb/            # local database
  cache/              # VFS file cache
    a3/               # sharded by first 2 chars of path-key hash
    tmp/              # in-progress full-file downloads
    index.db          # SQLite: node_id, export_path, mtime, size, block_map, last_access
  plugin_jobs.db      # SQLite: plugin event job queue and ack tracking
  certs/
    ca.crt            # control plane CA certificate
  bridges/            # cloud bridge credentials (control plane only)
    google_drive/
      credentials.enc

/run/mosaicfs/plugin-sockets/   # Unix domain sockets for socket plugins
  ai-summariser.sock            # bound by the plugin process, connected to by the agent
  fulltext-search.sock

/usr/lib/mosaicfs/plugins/      # plugin executables (Linux path)
  ai-summariser                 # executable or script
  exif-extractor
```

---

## Observability

### Logging

All components use the Rust `tracing` crate for structured logging with consistent key-value fields. Production deployments run at `INFO` level with runtime-adjustable log levels. Log files rotate at 50 MB, keeping 5 rotated files (250 MB total). Logs go to stderr in development and to a rolling file in production.

### Health Checks

Each agent exposes `GET /health` returning a JSON object with per-subsystem status (`pouchdb`, `replication`, `vfs_mount`, `watcher`, `transfer_server`, `plugins`). The `plugins` subsystem reports the status of each configured plugin: connected/disconnected for socket plugins, failed job counts and last-error for executable plugins. The control plane polls agents every 30 seconds. A node missing three consecutive checks is marked offline in its node document, which flows through to the web UI status dashboard.

### Error Classification

Errors are classified as:

- **Transient** — expected to resolve; retried with exponential backoff up to 60 seconds.
- **Permanent** — require intervention; immediately surfaced to `ERROR` log and web UI.
- **Soft** — partial success; logged but operation continues.

The last 50 errors per agent are stored in the agent's status document for quick access without log parsing.

---

## Cloud Service Bridges

### Bridge Interface

Each cloud bridge implements a common Rust trait with four methods: `list(path)` for directory contents, `stat(path)` for file metadata, `fetch(path)` returning an async byte stream, and `refresh_auth()` for token renewal. Bridges are pluggable — adding a new cloud service means implementing this trait and registering a new bridge type.

### Per-Service Notes

- **Google Drive** — Official REST API. OAuth2 with refresh tokens. Supports delta API for efficient incremental sync. Storage quota available via `storageQuota.limit` and `storageQuota.usage`.
- **Microsoft OneDrive** — Microsoft Graph API. Provides `quickXorHash`. OAuth2. Uses item IDs internally; bridge maintains a path-to-ID mapping. Storage quota available via `drive.quota`.
- **Backblaze B2** — S3-compatible API. `aws-sdk-rust` with custom endpoint. Object store model; directories are simulated from key prefixes. No quota ceiling; tracks used bytes only.
- **Amazon S3** — Native S3 API via `aws-sdk-rust`. Each bucket is registered as a separate bridge node. No quota ceiling; tracks used bytes only.
- **iCloud Drive** — No official third-party API. Accessed via the local `~/Library/Mobile Documents/` sync directory on macOS. Evicted files detected via the `com.apple.ubiquity.icloud-item-evicted` extended attribute and served via the control plane instead. Quota data not reliably available; `quota_available` is set to false.

### Polling Strategy

Google Drive and OneDrive support delta/changes APIs that return only what has changed since a stored cursor. These are polled frequently (every 60 seconds). Full listings are run periodically (every 5–10 minutes) as a consistency check. S3 and B2 have no push notification mechanism and rely on periodic full listings (every 10 minutes). Intervals are configurable.

---

## Web Interface

The web interface is a React single-page application. PouchDB syncs a filtered subset of the CouchDB database directly into the browser, so most data updates arrive via the live changes feed without explicit polling — file counts increment as agents index, node status badges update when heartbeats arrive, and rule match counts change as the rule engine processes new files. API calls are made via TanStack Query; shadcn/ui provides the component library.

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

**Node health strip.** A horizontal row of cards near the top of the page, one per node. Each card shows the node's friendly name, kind icon (physical agent or cloud bridge), online/offline/degraded status badge, and a compact storage utilization bar. Cards are color-coded by status: green for online, amber for degraded, red for offline or unreachable. Clicking any card navigates to that node's detail page.

**Quick-access search bar.** A large, prominent search input centered on the page. Submitting navigates to the Search page with the query pre-filled. Keyboard shortcut (`/` or `Cmd+K`) focuses it from anywhere on the dashboard.

**System totals.** A row of summary stats below the node strip: total indexed files, total virtual paths, total storage capacity across all physical nodes, total cloud storage used. These pull from the PouchDB replica and update live as agents index new files.

**Plugin widgets.** A row of widget cards below the system totals, one per plugin that advertises a `"dashboard_widget"` capability via `query_endpoints`. The control plane polls these nodes on a configurable interval (default 60 seconds) and caches the results. Each widget card shows the plugin's display name, a status badge derived from the widget's `status` field, and a compact set of key-value pairs from the widget's `data` object. Example: a fulltext-search widget might show "Index health: ✓", "Documents indexed: 47,203", "Last sync: 2 minutes ago." Widgets are rendered generically from the key-value data — no plugin-specific UI code. A widget with `status: "warning"` or `status: "error"` is visually highlighted amber or red. Clicking a widget navigates to the plugin's node detail page.

**Recent activity feed.** A scrollable list of the last 50 significant events across all nodes: files added, files deleted, rules evaluated, bridge syncs completed, errors. Each entry shows a timestamp, the originating node badge, and a short description. Filterable by node via a dropdown. The feed is not a firehose — it shows meaningful events, not every individual file change.

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

**List view.** A table of all nodes — physical agents and cloud bridges displayed together, distinguishable by a kind icon. Columns: friendly name, kind, platform, status badge, last heartbeat (relative time, e.g. "2 minutes ago"), indexed file count, and a compact storage utilization bar. A filter control lets the user show only physical nodes or only cloud bridges. Clicking any row opens the node detail page.

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

**Cloud bridge detail page.**

*Header:* Bridge name, bridge type (Google Drive, OneDrive, etc.), overall health badge, last sync timestamp.

*OAuth status panel:* Connected / Disconnected status with token expiry date. Re-authorize button initiates the OAuth flow via `GET /api/nodes/{node_id}/auth`. Revoke button calls `DELETE /api/nodes/{node_id}/auth`.

*Storage panel:* For quota-billed services, a used/total capacity bar with quota in GB. For consumption-billed services (S3, B2), used bytes only with no ceiling bar. For iCloud, an approximate figure with a note that official quota data is unavailable.

*Utilization trend chart:* Same as physical agent, but showing cloud storage used bytes over time.

*Sync controls:* Last sync time, next scheduled sync time, and a "Sync now" button calling `POST /api/nodes/{node_id}/sync`.

*Recent errors:* Same error table as physical agent detail.

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

**Cloud Bridges tab.** One card per cloud bridge node showing: bridge name, service type icon, connection status (Connected / Disconnected), token expiry date if connected. An "Authorize" or "Re-authorize" button initiates the OAuth flow in a popup window. A "Revoke access" link with confirmation disconnects the bridge.

**Plugins tab.** One card per plugin document that has a `settings_schema` declared, grouped by node. Each card shows the plugin's display name, the node it runs on as a node badge, and a form rendered from the `settings_schema`. Field types render as follows: `string` → text input, `number` → number input with optional min/max, `boolean` → toggle, `enum` → select dropdown, `secret` → password input that shows `••••••••` after initial save with an explicit "Change" button to reveal the field for editing. Each field shows its `description` as helper text below the input. Required fields are marked with an asterisk. Default values are shown as placeholder text when the field is empty.

Saving a card writes the new values to `settings` on the plugin document via `PATCH /api/nodes/{node_id}/plugins/{plugin_name}`. Changes replicate to the agent and take effect within seconds — the live changes feed means the plugin receives updated config on its next invocation without any restart. A success toast confirms the save; validation errors (required field missing, value out of range) are shown inline before the request is made.

Plugins without a `settings_schema` do not appear on this tab — they are configured via the raw JSON editor on the node detail page. A note at the bottom of the tab reads "Plugins without a settings schema are configured on the node detail page" with a link.

**General tab.** Configuration fields for: bridge polling intervals (one per bridge type, in seconds), VFS cache size limit (GB), utilization snapshot retention period (days), and nightly full crawl enable/disable toggle.

**About tab.** Instance information: MosaicFS version, setup date, control plane host. System totals: node count, indexed file count, active rule count, total virtual paths.

**Backup section.** Two download buttons side by side: "Download minimal backup" and "Download full backup". Clicking either calls `GET /api/system/backup?type=minimal` or `?type=full` and streams the JSON file as a download. The button labels include a file size estimate when available. A note below the buttons explains the difference: "Minimal: essential user data only (fast restore, small file). Full: complete database including history (disaster recovery)."

**Restore section.** Conditionally displayed based on `GET /api/system/backup/status`. If the database is empty (`{ empty: true }`), a file upload control appears with the label "Restore from backup". The user selects a JSON backup file, clicks "Restore", and the UI uploads it via `POST /api/system/restore`. A progress indicator shows during the upload and restore. On success, a banner appears prompting the user to restart all agents, with a "Restart all agents" button that calls `POST /api/system/reindex`. On error, the validation errors are shown inline. If the database is not empty, the restore section shows a disabled upload control with a tooltip: "Restore is only permitted into an empty database. To enable restore in a non-empty database, use the DELETE /api/system/data endpoint (requires --developer-mode)."

**System actions.** A "Trigger full reindex" button with a confirmation dialog, calling `POST /api/system/reindex`. A link to the project documentation.

---

## Open Questions

This section captures design decisions that were discussed but remain unresolved or deserve reconsideration before implementation.

### Virtual Directory and Label Event Hooks for Plugins

**Context:** Plugins currently subscribe to file lifecycle events (`file.added`, `file.modified`, `file.deleted`). During design, we considered additional event types: `vfs.directory.created`, `vfs.directory.modified`, `vfs.directory.deleted`, and `label.assigned`, `label.removed`, `label.rule.created`, `label.rule.deleted`.

**Question:** Should these be added to v1, or held for later? 

**Arguments for deferral:** No concrete plugin use case exists yet. Virtual directory changes are user-driven and infrequent — a plugin reacting to them is more like a webhook than an annotation pipeline. Label hooks feel like workflow automation ("when labelled X, do Y") which is a different category of extension.

**Arguments for inclusion:** A hypothetical "sync to external DMS when labelled `archive`" plugin is genuinely useful. A plugin maintaining an external catalog mirroring the virtual tree structure could use directory events.

**Current state:** Deferred from v1. Easy to add later without breaking changes.

---

### Plugin Query Result Streaming

**Context:** The `POST /api/query` endpoint fans out to all nodes advertising a capability, collects responses, and returns them as an array. Currently specified as gather-then-return.

**Question:** Should query results stream back to the browser as nodes respond, or wait until all nodes have responded (or timed out)?

**Tradeoffs:**
- Gather-then-return: Simple, predictable, works for v1 where plugin-agents are co-located with the control plane on fast local network
- Streaming (server-sent events or chunked JSON): Faster perceived latency, more complex to implement, better for geographically distributed nodes

**Current state:** v1 uses gather-then-return. Note added that streaming could be a future improvement.

---

### Bridge Node Storage: Option A vs Option B Threshold

**Context:** Bridge nodes can use Option A (one file per record, direct Tier 1 serving) or Option B (aggregate SQLite storage with Tier 5 materialize on demand).

**Question:** At what point should a deployment switch from Option A to Option B? Is there a file count threshold, or should it always be the plugin author's choice?

**Considerations:**
- Inode exhaustion on ext4 starts becoming a concern around 500K-1M small files
- Date-based sharding helps but doesn't eliminate the problem
- Option B adds materialize latency on first access, but VFS cache makes subsequent accesses fast
- Plugin implementation complexity: Option B requires maintaining SQLite schema and materialize logic; Option A is just "write .eml files"

**Current state:** Option A recommended for v1, with Option B documented as the upgrade path. No specific guidance on when to switch. Volume formatting with high inode count (`mkfs.ext4 -N 2000000`) mentioned as a mitigation.

---

### Scheduled Automatic Backups

**Context:** Backup and restore are on-demand in v1 via the REST API and Settings page. User downloads the JSON file and stores it wherever they want.

**Question:** Should MosaicFS support scheduled automatic backups to a configured destination (S3 bucket, local directory, NAS share)?

**Considerations:**
- Natural extension of the backup feature
- Requires destination configuration (credentials, paths)
- Rotation policy (keep last N backups, delete older than X days)
- Notification on backup failure

**Current state:** Explicitly noted as "not implemented in v1 but are a natural future extension."

---

### Global Settings Scope for Bridge Plugins

**Context:** Bridge plugins (email-fetch, caldav-sync) might be deployed on multiple nodes. Settings like "Gmail API endpoint URL" or "Meilisearch connection string" are likely the same across all instances of a plugin.

**Question:** Should plugin settings support a `settings_scope` field distinguishing between `"node"` (per-node configuration) and `"global"` (shared across all instances)?

**Considerations:**
- Per-node is consistent with the current model but tedious if every node needs identical settings
- Global settings would be stored in a synthetic `plugin::{plugin_name}` document without a node ID
- Agent falls back to global settings when no node-specific override exists
- Adds complexity to the settings merge logic

**Current state:** Noted as "worth noting as a future direction but probably not v1" in the conversation. Not documented in the architecture.

---

### Networked Socket Plugins (TCP)

**Context:** Socket plugins currently use Unix domain sockets. The architecture notes that "the Unix socket model extends naturally to networked plugins: replacing the socket path with a TCP address would allow plugins to run on a different machine from the agent."

**Question:** Is this a planned v2 feature, or just a noted possibility?

**Considerations:**
- Enables plugin deployment on dedicated hardware (GPU servers for AI plugins)
- Requires rethinking security — TCP sockets need authentication, Unix sockets inherit filesystem permissions
- Event delivery over TCP needs TLS

**Current state:** Noted in Future Directions but not committed to.

---

### Push-based Plugin Notifications

**Context:** Socket plugins are polled for health status on a configurable interval (default 5 minutes). Plugins can report notifications in the health check response.

**Question:** Should socket plugins be able to push notifications at arbitrary times, rather than waiting for the next health check poll?

**Tradeoffs:**
- Pull (current): Simple agent implementation, tolerates latency, plugin can't overwhelm agent
- Push (future): Lower latency for urgent notifications, requires unsolicited message handling on socket

**Current state:** V1 uses pull. Noted that "the socket remains available for a future push extension — an unsolicited `{ \"type\": \"notification\" }` message from the plugin would require only a small addition to the inbound message handler."

---

*MosaicFS Architecture Document v0.1 — Subject to revision*
