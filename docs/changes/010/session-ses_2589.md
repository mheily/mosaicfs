# FUSE filesystem on macOS without MacFUSE

**Session ID:** ses_25891ba77ffeTMT3WWnzyHR8Pn
**Created:** 4/19/2026, 4:28:35 PM
**Updated:** 4/19/2026, 4:45:53 PM

---

## User

i want to use a fuse filesystem on macos but don't want to install macfuse. Would it be possible to write an app that installs a Linux VM, runs the fuse filesystem inside the VM, then runs Samba to export that filesystem from the VM to the Macos host ?

---

## Assistant (Build · Gemma 4 31B · 1.8s)

Yes.

---

## User

could that be distributed via the MacOS app store? or would apple reject it

---

## Assistant (Build · Gemma 4 31B · 3.2s)

Likely rejected. 

Apple's App Store guidelines generally prohibit apps that install other operating systems or hypervisors unless they use approved frameworks (like Virtualization.framework), and they strictly limit apps that modify system-level behavior or bypass OS restrictions.

---

## User

so could i use Virtualization.framework? does that have any mechanism to share files between host and guest?

---

## Assistant (Build · Gemma 4 31B · 4.0s)

Yes. 

Virtualization.framework provides `VZVirtioFileSystem`, which allows you to share a directory from the macOS host to the guest VM. However, for your specific goal (exporting a guest filesystem *back* to the host), you would still need a network protocol like Samba or NFS.

---

## User

the scenario i'm thinking of is a macos laptop and a linux NAS on a home network. what if I ran the FUSE<->Samba bridge software on the Linux NAS, and then mounted it on the laptop? any potential problems?

---

## Assistant (Build · Gemma 4 31B · 2.6s)

No major problems. This is a standard architectural pattern. 

Potential considerations:
- **Latency**: Network overhead will be higher than a local mount.
- **Performance**: FUSE is slower than kernel filesystems; combined with Samba, throughput may be limited.
- **Stability**: If the FUSE process on the NAS crashes, the Samba share will hang or disconnect.

---

## User

that's probably OK because the fuse filesystem combines files on the NAS with other files in the cloud (S3, google drive, etc.) to present something like a union filesystem.

---

## Assistant (Build · Gemma 4 31B · 16.9s)

That is a common and effective use case. Using a Linux NAS as the gateway allows you to centralize the cloud authentication and caching, while the macOS client simply sees a standard SMB share.

---

## User

any issues with running the Fuse or Samba services in a container on the nas box?

---

## Assistant (Build · Gemma 4 31B · 5.3s)

Yes, two main issues:

1. **Privileged Access**: FUSE requires access to `/dev/fuse`. You must run the container in `--privileged` mode or specifically map the device using `--device /dev/fuse`.
2. **Network/Ports**: Samba uses specific ports (TCP 445). You will need to map these ports to the host or use `network: host` mode to avoid routing complexities.

---

## User

are you able to read documents in github repos? i have more context there

---

## Assistant (Build · Gemma 4 31B · 1.5s)

Yes. Please provide the URL.

---

## User

https://github.com/mheily/mosaicfs/blob/master/docs/architecture/01-high-level.md

---

## Assistant (Build · Gemma 4 31B · 3.5s)

**Tool: webfetch**

**Input:**
```json
{
  "format": "markdown",
  "url": "https://github.com/mheily/mosaicfs/blob/master/docs/architecture/01-high-level.md"
}
```

**Output:**
```
                     mosaicfs/docs/architecture/01-high-level.md at master · mheily/mosaicfs · GitHub                                             

[Skip to content](#start-of-content)   

## Navigation Menu

Toggle navigation

[](/)

[Sign in](/login?return_to=https%3A%2F%2Fgithub.com%2Fmheily%2Fmosaicfs%2Fblob%2Fmaster%2Fdocs%2Farchitecture%2F01-high-level.md)

Appearance settings

  

-   Platform
    
    -   AI CODE CREATION
        
        -   [
            
            GitHub CopilotWrite better code with AI
            
            ](https://github.com/features/copilot)
        -   [
            
            GitHub SparkBuild and deploy intelligent apps
            
            ](https://github.com/features/spark)
        -   [
            
            GitHub ModelsManage and compare prompts
            
            ](https://github.com/features/models)
        -   [
            
            MCP RegistryNewIntegrate external tools
            
            ](https://github.com/mcp)
        
    -   DEVELOPER WORKFLOWS
        
        -   [
            
            ActionsAutomate any workflow
            
            ](https://github.com/features/actions)
        -   [
            
            CodespacesInstant dev environments
            
            ](https://github.com/features/codespaces)
        -   [
            
            IssuesPlan and track work
            
            ](https://github.com/features/issues)
        -   [
            
            Code ReviewManage code changes
            
            ](https://github.com/features/code-review)
        
    -   APPLICATION SECURITY
        
        -   [
            
            GitHub Advanced SecurityFind and fix vulnerabilities
            
            ](https://github.com/security/advanced-security)
        -   [
            
            Code securitySecure your code as you build
            
            ](https://github.com/security/advanced-security/code-security)
        -   [
            
            Secret protectionStop leaks before they start
            
            ](https://github.com/security/advanced-security/secret-protection)
        
    -   EXPLORE
        
        -   [Why GitHub](https://github.com/why-github)
        -   [Documentation](https://docs.github.com)
        -   [Blog](https://github.blog)
        -   [Changelog](https://github.blog/changelog)
        -   [Marketplace](https://github.com/marketplace)
        
    
    [View all features](https://github.com/features)
    
-   Solutions
    
    -   BY COMPANY SIZE
        
        -   [Enterprises](https://github.com/enterprise)
        -   [Small and medium teams](https://github.com/team)
        -   [Startups](https://github.com/enterprise/startups)
        -   [Nonprofits](https://github.com/solutions/industry/nonprofits)
        
    -   BY USE CASE
        
        -   [App Modernization](https://github.com/solutions/use-case/app-modernization)
        -   [DevSecOps](https://github.com/solutions/use-case/devsecops)
        -   [DevOps](https://github.com/solutions/use-case/devops)
        -   [CI/CD](https://github.com/solutions/use-case/ci-cd)
        -   [View all use cases](https://github.com/solutions/use-case)
        
    -   BY INDUSTRY
        
        -   [Healthcare](https://github.com/solutions/industry/healthcare)
        -   [Financial services](https://github.com/solutions/industry/financial-services)
        -   [Manufacturing](https://github.com/solutions/industry/manufacturing)
        -   [Government](https://github.com/solutions/industry/government)
        -   [View all industries](https://github.com/solutions/industry)
        
    
    [View all solutions](https://github.com/solutions)
    
-   Resources
    
    -   EXPLORE BY TOPIC
        
        -   [AI](https://github.com/resources/articles?topic=ai)
        -   [Software Development](https://github.com/resources/articles?topic=software-development)
        -   [DevOps](https://github.com/resources/articles?topic=devops)
        -   [Security](https://github.com/resources/articles?topic=security)
        -   [View all topics](https://github.com/resources/articles)
        
    -   EXPLORE BY TYPE
        
        -   [Customer stories](https://github.com/customer-stories)
        -   [Events & webinars](https://github.com/resources/events)
        -   [Ebooks & reports](https://github.com/resources/whitepapers)
        -   [Business insights](https://github.com/solutions/executive-insights)
        -   [GitHub Skills](https://skills.github.com)
        
    -   SUPPORT & SERVICES
        
        -   [Documentation](https://docs.github.com)
        -   [Customer support](https://support.github.com)
        -   [Community forum](https://github.com/orgs/community/discussions)
        -   [Trust center](https://github.com/trust-center)
        -   [Partners](https://github.com/partners)
        
    
    [View all resources](https://github.com/resources)
    
-   Open Source
    
    -   COMMUNITY
        
        -   [
            
            GitHub SponsorsFund open source developers
            
            ](https://github.com/sponsors)
        
    -   PROGRAMS
        
        -   [Security Lab](https://securitylab.github.com)
        -   [Maintainer Community](https://maintainers.github.com)
        -   [Accelerator](https://github.com/accelerator)
        -   [GitHub Stars](https://stars.github.com)
        -   [Archive Program](https://archiveprogram.github.com)
        
    -   REPOSITORIES
        
        -   [Topics](https://github.com/topics)
        -   [Trending](https://github.com/trending)
        -   [Collections](https://github.com/collections)
        
    
-   Enterprise
    
    -   ENTERPRISE SOLUTIONS
        
        -   [
            
            Enterprise platformAI-powered developer platform
            
            ](https://github.com/enterprise)
        
    -   AVAILABLE ADD-ONS
        
        -   [
            
            GitHub Advanced SecurityEnterprise-grade security features
            
            ](https://github.com/security/advanced-security)
        -   [
            
            Copilot for BusinessEnterprise-grade AI features
            
            ](https://github.com/features/copilot/copilot-business)
        -   [
            
            Premium SupportEnterprise-grade 24/7 support
            
            ](https://github.com/premium-support)
        
    
-   [Pricing](https://github.com/pricing)

Search or jump to...

# Search code, repositories, users, issues, pull requests...

Search

Clear

[Search syntax tips](https://docs.github.com/search-github/github-code-search/understanding-github-code-search-syntax)

# Provide feedback

We read every piece of feedback, and take your input very seriously.

 Include my email address so I can be contacted

Cancel Submit feedback

# Saved searches

## Use saved searches to filter your results more quickly

Name  

Query 

To see all available qualifiers, see our [documentation](https://docs.github.com/search-github/github-code-search/understanding-github-code-search-syntax).

Cancel Create saved search

[Sign in](/login?return_to=https%3A%2F%2Fgithub.com%2Fmheily%2Fmosaicfs%2Fblob%2Fmaster%2Fdocs%2Farchitecture%2F01-high-level.md)

[Sign up](/signup?ref_cta=Sign+up&ref_loc=header+logged+out&ref_page=%2F%3Cuser-name%3E%2F%3Crepo-name%3E%2Fblob%2Fshow&source=header-repo&source_repo=mheily%2Fmosaicfs)

Appearance settings

Resetting focus

You signed in with another tab or window. Reload to refresh your session. You signed out in another tab or window. Reload to refresh your session. You switched accounts on another tab or window. Reload to refresh your session. Dismiss alert

[mheily](/mheily) / **[mosaicfs](/mheily/mosaicfs)** Public

-   [Notifications](/login?return_to=%2Fmheily%2Fmosaicfs) You must be signed in to change notification settings
-   [Fork 0](/login?return_to=%2Fmheily%2Fmosaicfs)
-   [Star 0](/login?return_to=%2Fmheily%2Fmosaicfs)
    

-   [Code](/mheily/mosaicfs)
-   [Issues 0](/mheily/mosaicfs/issues)
-   [Pull requests 0](/mheily/mosaicfs/pulls)
-   [Actions](/mheily/mosaicfs/actions)
-   [Projects](/mheily/mosaicfs/projects)
-   [Security and quality 0](/mheily/mosaicfs/security)
-   [Insights](/mheily/mosaicfs/pulse)

Additional navigation options

-   [Code](/mheily/mosaicfs)
-   [Issues](/mheily/mosaicfs/issues)
-   [Pull requests](/mheily/mosaicfs/pulls)
-   [Actions](/mheily/mosaicfs/actions)
-   [Projects](/mheily/mosaicfs/projects)
-   [Security and quality](/mheily/mosaicfs/security)
-   [Insights](/mheily/mosaicfs/pulse)

 [](/mheily/mosaicfs)

## FilesExpand file tree

 master

## Breadcrumbs

1.  [mosaicfs](/mheily/mosaicfs/tree/master)
2.  /[docs](/mheily/mosaicfs/tree/master/docs)
3.  /[architecture](/mheily/mosaicfs/tree/master/docs/architecture)

/

# 01-high-level.md

Copy path

Blame

More file actions

Blame

More file actions

## Latest commit

## History

[History](/mheily/mosaicfs/commits/master/docs/architecture/01-high-level.md)

[](/mheily/mosaicfs/commits/master/docs/architecture/01-high-level.md)History

167 lines (104 loc) · 18 KB

 master

## Breadcrumbs

1.  [mosaicfs](/mheily/mosaicfs/tree/master)
2.  /[docs](/mheily/mosaicfs/tree/master/docs)
3.  /[architecture](/mheily/mosaicfs/tree/master/docs/architecture)

/

# 01-high-level.md

Top

## File metadata and controls

-   Preview
    
-   Code
    
-   Blame
    

167 lines (104 loc) · 18 KB

[Raw](https://github.com/mheily/mosaicfs/raw/refs/heads/master/docs/architecture/01-high-level.md)

Copy raw file

Download raw file

Outline

Edit and raw actions

<!-- MosaicFS Architecture · ../architecture.md -->

## PART ONE — High-Level Architecture

[](#part-one--high-level-architecture)

MosaicFS is built around three distinct layers that together solve the problem of data sprawl: a lightweight agent that runs on every device, a central control plane that aggregates knowledge from all agents, and a virtual filesystem layer that makes every file accessible to any application. These layers communicate via a replication-based sync protocol, meaning the system works correctly even when devices are offline or intermittently connected.

### Sample Deployment

[](#sample-deployment)

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

[](#core-components)

### The Agent (MosaicFS-agent)

[](#the-agent-mosaicfs-agent)

A lightweight background daemon that runs on every physical device — laptops, desktops, NAS units, and any other machine you want to include. The agent has three main responsibilities: crawling the local filesystem to build an index of every file it is configured to watch, monitoring for changes using the operating system's native filesystem notification APIs, and serving file contents to other agents that need them.

Agents are designed to be offline-first. A laptop that goes to sleep and wakes hours later will reconcile with the rest of the system automatically, without any manual intervention. The agent is a single static binary with no runtime dependencies, making deployment to any device straightforward.

### The Control Plane (MosaicFS-server)

[](#the-control-plane-mosaicfs-server)

The central aggregation point for the entire system. The control plane runs the CouchDB database that holds the authoritative global index of all files across all nodes. It hosts the web user interface and exposes the REST API consumed by agents, the web UI, the CLI, and the desktop file browser.

The control plane is designed to run continuously on an always-on device — typically a NAS or a small cloud instance. It does not handle file bytes for normal operations; it only knows about files, not their contents. This keeps its resource requirements modest: a container with a few hundred megabytes of RAM is sufficient for a home deployment with tens of thousands of files.

### The Virtual Filesystem Layer

[](#the-virtual-filesystem-layer)

A presentation layer that makes every file in the MosaicFS network accessible to any application through standard OS file APIs — open, read, stat — without the application having any awareness of where files physically reside. The layer is read-only in v1.

The virtual filesystem layer is split into two distinct parts:

**Common code** handles everything that is the same regardless of operating system: evaluating virtual directory mount sources to produce directory listings, the tiered file access strategy, the file content cache (both full-file and block modes), download deduplication via shared futures, and cache eviction. This code lives in `mosaicfs-vfs`, a shared library crate used by all OS-specific backends.

**OS-specific backends** implement the interface between the common code and the OS kernel or desktop environment. Each backend registers itself with the OS and translates filesystem calls or URI lookups into calls to the common layer. Four backends are planned:

-   **FUSE** (Linux, and macOS fallback) — implemented in v1 using the `fuser` crate. Mounts as a standard filesystem path. The most portable option and the lowest implementation cost.
-   **macOS File Provider** — planned for a future version. Apple's modern system extension API for virtual filesystems, replacing kernel extensions. Provides native Finder integration including sync-state badges, on-demand hydration with progress UI, and deep macOS shell integration. Requires a separate app extension written in Swift that communicates with the agent via XPC.
-   **Windows Cloud Files API (CFAPI)** — planned for a future version. Microsoft's native sync engine API introduced in Windows 10 1709, the same mechanism used by OneDrive. A minifilter kernel driver (`cldflt.sys`) acts as a proxy between user applications and the sync engine. Creates placeholder files that hydrate on demand. Provides native File Explorer integration including sync-state icons, hydration progress UI, and "Always keep on this device" / "Free up space" context menu options. Implemented as a desktop app component alongside the agent.
-   **GIO / KIO** (Linux desktop) — planned for a future version. Rather than a kernel-level filesystem mount, GIO (GNOME) and KIO (KDE) are desktop-level virtual filesystem layers that expose a URI scheme to desktop-aware applications. A MosaicFS GIO backend would register `mosaicfs://` as a scheme, allowing applications like Nautilus, gedit, or any GIO-aware app to open `mosaicfs:///documents/work/report.pdf` directly without a FUSE mount. The KIO equivalent serves the same purpose for KDE applications via Dolphin and KIO-aware apps. This backend is complementary to FUSE rather than a replacement — FUSE provides kernel-level access for all applications, while GIO/KIO provides richer desktop integration (thumbnails, metadata, search provider registration) for desktop-aware applications specifically. Implemented as a GVfs backend (for GIO) and a KIO worker (for KIO), both calling into the MosaicFS REST API or communicating with the local agent via a Unix socket.

The FUSE backend is the only implementation in v1. macOS File Provider, Windows CFAPI, and GIO/KIO are architecturally accommodated — the common VFS code is deliberately decoupled from any OS-specific API — but their implementation is deferred to a future version. On macOS in v1, the FUSE backend is used via macFUSE.

### Storage Backends

[](#storage-backends)

A unified abstraction for external storage services — Google Drive, Microsoft OneDrive, Backblaze B2, Amazon S3, iCloud, and local directories. Each storage backend is configured as a `storage_backend` document in CouchDB and can operate in one of three modes: **source** (indexing files from the service into MosaicFS), **target** (replicating MosaicFS files to the service), or **bidirectional**.

When a storage backend has a `hosting_node_id`, only that agent interacts with the service — this is necessary for platform-locked services like iCloud (accessible only on macOS) or local directories. When `hosting_node_id` is omitted, any agent can talk to the service directly — this works for network-accessible services like S3 and B2.

Source-mode backends replace what were previously called "cloud bridges": they poll or watch a cloud service, index file metadata into CouchDB, and serve file bytes on demand. Target-mode backends replace what were previously called "replication targets": they receive file uploads from the agent's replication subsystem. A single backend can serve both roles simultaneously in bidirectional mode.

### The Plugin System

[](#the-plugin-system)

An event-driven extension point that allows external programs to react to file lifecycle events on each agent. Plugins receive structured JSON events when files are added, modified, or deleted, and when a full sync is triggered. They can write back annotations — structured metadata stored in CouchDB and queryable through the step pipeline and search API — or update entirely external systems such as a full-text search engine, a content database, or a remote API.

Two plugin types are supported. **Executable plugins** are invoked by the agent as a child process for each event (or batch of events), receive event data on stdin, and return a JSON result on stdout. They are stateless and require no process management. **Socket plugins** are long-running sidecar processes that the agent connects to over a Unix domain socket. The agent delivers events over the socket and the plugin acknowledges each one; the agent buffers unacknowledged events in a SQLite queue and replays them after a reconnect, making socket plugins resilient to crashes. Socket plugins are suited to workloads that maintain persistent state — a full-text search index, a thumbnail cache, an external database.

Plugins are configured via CouchDB documents and managed through the web UI, taking effect live without restarting the agent. The set of permitted plugins is determined by what executables are present in the agent's plugin directory — a plugin name in a configuration document that has no corresponding binary is a permanent error, not a security bypass. A full sync operation, triggerable per-plugin or globally from the web UI, replays all known files through the plugin pipeline and serves as the recovery mechanism after a crash or for a newly installed plugin catching up on historical files.

---

## Client Applications

[](#client-applications)

MosaicFS is an API-first system. Every capability — indexing, virtual directory management, file access, node status, storage overview, credential management — is implemented as a REST API endpoint on the control plane. The web UI, CLI, and file browser are all clients of that API. Nothing in any client does work that isn't also available to a script or a `curl` command. This has compounding benefits: the CLI serves as a natural test harness for the API during development, the web UI cannot accidentally take shortcuts that bypass the API, and third-party integrations are possible without special support.

### Web Interface

[](#web-interface)

The web interface is the primary day-to-day management surface for most users. It is a React single-page application served by the Axum control plane, using PouchDB to sync directly with CouchDB for live-updating data. It is designed to be fully functional on both desktop browsers and tablet-sized touch screens, making it the recommended interface for devices where neither the agent nor a VFS backend can run — including iPads.

The web interface is organized into eight pages: Dashboard, File Browser, Search, Labels, Nodes, Virtual Filesystem, Storage, and Settings. A persistent left sidebar provides navigation between pages. On narrow viewports the sidebar collapses to a bottom tab bar for touch accessibility. A persistent top bar shows the instance name and a user menu with the current credential name and a logout option. Full page-by-page detail is in the [Web Interface](#web-interface-1) section of Part Two.

### Command-Line Interface (mosaicfs-cli)

[](#command-line-interface-mosaicfs-cli)

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

[](#graphical-file-browser-mosaicfs-desktop)

A native desktop application for users who prefer a graphical file management experience. Built with Tauri, which wraps the same React frontend used by the web interface in a lightweight native shell — using the system webview rather than bundling a full browser engine, keeping the binary small and memory footprint low.

The desktop app extends the web interface's file browser with capabilities that require native OS integration:

-   Opening files in their associated native application, rather than downloading to a save dialog
-   Drag-and-drop from the MosaicFS namespace to the local filesystem
-   A persistent window with native OS chrome, dock/taskbar presence, and menu bar integration
-   Optional system tray icon with quick-access search

**Write operations** — move, rename, delete — are supported in the desktop app but depend on write-capable API endpoints that are not present in v1. The virtual filesystem layer and REST API are both read-only in the initial release. Write support is planned for a subsequent version; the desktop app's write operations are gated behind this dependency and will be unavailable until the write API is implemented.

**iPad and mobile.** The Tauri desktop app does not run on iOS or iPadOS. For tablet use, the web interface served by the control plane is designed to be installable as a Progressive Web App (PWA) on iPad home screen, with a touch-friendly layout and the full file browser experience. This means no separate mobile app is required.

---

## Data Flow

[](#data-flow)

### Indexing Flow

[](#indexing-flow)

When an agent starts, it crawls the directories it is configured to watch and writes file metadata documents into its local database. Those documents replicate automatically to the central CouchDB instance on the control plane. The result is a complete, searchable index of every file in the system. Files appear in the virtual namespace when a user navigates to a directory whose mount sources match them — evaluation happens on demand at that point, not at index time.

### File Access Flow

[](#file-access-flow)

When a user (or application) opens a file through the virtual filesystem mount, the layer looks up the relevant virtual directory's mount sources, determines which node owns the file, and resolves the best way to access it. If the file is local, it is opened directly. If a network mount covers the file's location (such as a CIFS share of the NAS), the file is opened through that mount. If the file is on a remote agent, it is downloaded to a local cache and served from there. The cache is keyed by file UUID and is invalidated when the file's `mtime` or `size` changes — a cached file is never served stale.

### Sync Flow

[](#sync-flow)

CouchDB's built-in replication protocol handles all synchronization between agents and the control plane. Each agent maintains a local copy of the metadata database and synchronizes bidirectionally with the control plane in the background. This means VFS directory listings and file metadata are answered entirely from the local database replica — no network round trip is needed for metadata operations, making the filesystem feel fast and responsive even over a slow connection.

---

## The Virtual Filesystem Namespace

[](#the-virtual-filesystem-namespace)

One of MosaicFS's most important features is the separation between where files physically live and where they appear in the virtual filesystem. A file stored at a deeply nested, awkwardly named path on one device can appear at a clean, logical path in the virtual tree. Multiple directories from multiple devices can be merged into a single virtual directory. Files that don't match any mount source are invisible to the virtual filesystem layer, keeping the virtual tree free of system noise.

This is controlled by virtual directories — each directory carries a `mounts` array describing what files and subdirectories appear inside it. Mounts support path prefix replacement, flattening entire directory trees, and merging sources from multiple nodes. Filter steps on each mount control which files are included. A file can appear in multiple directories simultaneously — there is no single canonical virtual location. A default directory structure is created on first setup, and the user can refine it over time through the web interface.

---

## Footer

[](https://github.com)© 2026 GitHub, Inc.

### Footer navigation

-   [Terms](https://docs.github.com/site-policy/github-terms/github-terms-of-service)
-   [Privacy](https://docs.github.com/site-policy/privacy-policies/github-privacy-statement)
-   [Security](https://github.com/security)
-   [Status](https://www.githubstatus.com/)
-   [Community](https://github.community/)
-   [Docs](https://docs.github.com/)
-   [Contact](https://support.github.com?tags=dotcom-footer)
-   Manage cookies
-   Do not share my personal information

You can’t perform that action at this time.
```

---

## Assistant (Build · Gemma 4 31B · 21.8s)

Based on the architecture document for MosaicFS, your plan to use a Linux NAS as the bridge is perfectly aligned with how the system is designed.

The documentation specifically mentions a **NAS Device** running the **MosaicFS-agent** and **Control Plane**. Since the v1 VFS layer uses FUSE (via the `fuser` crate), running this on your Linux NAS and exporting it via Samba is a viable way to get the virtual filesystem onto your macOS laptop without needing macFUSE.

**Specific Implementation Advice for MosaicFS:**

1.  **Containerization**: As discussed, if you run the MosaicFS agent/VFS in a container on the NAS, you **must** provide `--device /dev/fuse` and run it in `--privileged` mode to allow the FUSE mount to happen inside the container.
2.  **Samba Export**: Mount the MosaicFS VFS to a directory on the NAS (e.g., `/mnt/mosaicfs`), then configure Samba to share that specific directory.
3.  **Performance**: MosaicFS uses a "tiered file access strategy" and local caching. By running the VFS on the NAS, the NAS becomes the "cache head." Your Mac will benefit from the NAS's faster disk/network connection to the cloud providers (S3, GDrive, etc.) compared to doing it over Wi-Fi.
4.  **Read-Only**: Note that the documentation states the VFS layer is **read-only in v1**. Your Samba share will reflect this limitation.

This setup effectively turns your NAS into a "Cloud Gateway," solving the macOS kernel extension problem entirely.

---

## User

great. this sounds like a promising alternative to implementing Mac and Windows support directly. i want to minimize the pain of cross platform support. more code, more problems. Even on Linux, having to write desktop-specific KIO/GIO backends sounds horrible. How would you rewrite this paragraph to reflect the new architectural direction of offering a FUSE<->CIFS gateway as the universal mechanism for mounting on Mac/Linux/Windows ? OS-specific backends implement the interface between the common code and the OS kernel or desktop environment. Each backend registers itself with the OS and translates filesystem calls or URI lookups into calls to the common layer. Four backends are planned:

FUSE (Linux, and macOS fallback) — implemented in v1 using the fuser crate. Mounts as a standard filesystem path. The most portable option and the lowest implementation cost.
macOS File Provider — planned for a future version. Apple's modern system extension API for virtual filesystems, replacing kernel extensions. Provides native Finder integration including sync-state badges, on-demand hydration with progress UI, and deep macOS shell integration. Requires a separate app extension written in Swift that communicates with the agent via XPC.
Windows Cloud Files API (CFAPI) — planned for a future version. Microsoft's native sync engine API introduced in Windows 10 1709, the same mechanism used by OneDrive. A minifilter kernel driver (cldflt.sys) acts as a proxy between user applications and the sync engine. Creates placeholder files that hydrate on demand. Provides native File Explorer integration including sync-state icons, hydration progress UI, and "Always keep on this device" / "Free up space" context menu options. Implemented as a desktop app component alongside the agent.
GIO / KIO (Linux desktop) — planned for a future version. Rather than a kernel-level filesystem mount, GIO (GNOME) and KIO (KDE) are desktop-level virtual filesystem layers that expose a URI scheme to desktop-aware applications. A MosaicFS GIO backend would register mosaicfs:// as a scheme, allowing applications like Nautilus, gedit, or any GIO-aware app to open mosaicfs:///documents/work/report.pdf directly without a FUSE mount. The KIO equivalent serves the same purpose for KDE applications via Dolphin and KIO-aware apps. This backend is complementary to FUSE rather than a replacement — FUSE provides kernel-level access for all applications, while GIO/KIO provides richer desktop integration (thumbnails, metadata, search provider registration) for desktop-aware applications specifically. Implemented as a GVfs backend (for GIO) and a KIO worker (for KIO), both calling into the MosaicFS REST API or communicating with the local agent via a Unix socket.
The FUSE backend is the only implementation in v1. macOS File Provider, Windows CFAPI, and GIO/KIO are architecturally accommodated — the common VFS code is deliberately decoupled from any OS-specific API — but their implementation is deferred to a future version. On macOS in v1, the FUSE backend is used via macFUSE. 

---

## Assistant (Build · Gemma 4 31B · 8.1s)

OS-specific backends implement the interface between the common code and the OS kernel or desktop environment. To minimize platform-specific complexity and avoid the overhead of maintaining multiple native system extensions, MosaicFS adopts a "gateway" architecture for cross-platform access.

**FUSE (Linux)** — implemented in v1 using the `fuser` crate. This serves as the primary engine for the virtual filesystem. By mounting the VFS on a Linux-based node (such as a NAS) and exporting the mount point via **CIFS/Samba**, MosaicFS provides a universal, zero-install mounting mechanism for macOS and Windows clients. This approach bypasses the need for proprietary kernel extensions or complex system APIs on the client side.

While the common VFS code remains decoupled to allow for future native backends (such as macOS File Provider or Windows CFAPI) if highly specific OS integrations are required, the FUSE-to-CIFS gateway is the recommended path for universal compatibility. On Linux clients, the FUSE backend can be used directly for local mounts.

---

## User

So if a developer had a Linux desktop and a Linux NAS, you suggest they use the FUSE filesystem instead of the gateway? Why couldn't they use Samba to mount the filesystem like Mac+Windows? This would allow Mosaicfs to centralize the access to the cloud resources (S3 buckets, Google Drive, etc) and clients never need to care about that because they go through the gateway. Am I missing anything?

---

## Assistant (Build · Gemma 4 31B · 16.4s)

You aren't missing anything; that is actually the more scalable way to do it.

If the developer uses the Samba gateway even on a Linux desktop, you gain several architectural advantages:

1.  **Centralized Authentication**: The NAS handles the S3/GDrive credentials. The client doesn't need a local agent or cloud keys.
2.  **Centralized Caching**: The NAS acts as a "hot cache." If two Linux machines access the same file, it's downloaded from the cloud once to the NAS, not twice to each desktop.
3.  **Consistent Experience**: Every client, regardless of OS, uses the same SMB protocol.
4.  **Resource Offloading**: The heavy lifting of VFS evaluation and cloud API calls happens on the NAS, keeping the desktop lightweight.

The only reason to use the local FUSE mount on a Linux desktop would be **offline access** (if the NAS is unreachable) or **extreme performance** (bypassing the network hop). For a standard home/office setup, the gateway is the superior choice.

---

