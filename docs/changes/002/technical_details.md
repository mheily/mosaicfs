# Technical Details: Evaluating fs123 as a File Reading Replacement

## Current State Summary

### MosaicFS File Reading Surface Area

MosaicFS has **three overlapping file access paths** that collectively form its
"internal file reading API":

#### 1. REST API File Access (mosaicfs-server)

**Files:** `mosaicfs-server/src/handlers/files.rs`, `mosaicfs-server/src/routes.rs`

7 endpoints under `/api/files/*`:
- `GET /api/files` — list files (CouchDB query, filtered by node/status/mime)
- `GET /api/files/{file_id}` — get file document
- `GET /api/files/by-path` — find file by export_path (CouchDB `_find`)
- `GET /api/files/{file_id}/content` — JWT-authenticated content download
- `GET /api/files/{file_id}/token` — issue HMAC-signed download token
- `GET /api/files/{file_id}/download` — token-authenticated content download
- `GET /api/files/{file_id}/access` — access tracking record

Content serving (`serve_file_content` in `files.rs:254`) works as:
1. Look up file document in CouchDB to get `source.export_path` and `source.node_id`
2. If file is local on disk: open and stream directly (with HTTP Range support)
3. If file is remote: proxy to the source agent's file server via `proxy_to_agent()`

#### 2. Agent File Server (mosaicfs-agent)

**Files:** `mosaicfs-agent/src/file_server.rs`, `mosaicfs-agent/src/main.rs`

Single endpoint: `GET /internal/files/content?path=...&start=...&end=...`
- Bearer-token authenticated
- Serves raw file bytes from the agent's local filesystem
- Supports range requests
- Registered in CouchDB as `node.file_server_url` at startup (port 8444)
- Used as the proxy target when the server's `serve_file_content` determines
  the file is on a different node

#### 3. VFS FUSE Layer (mosaicfs-vfs)

**Files:** `mosaicfs-vfs/src/fuse_fs.rs`, `mosaicfs-vfs/src/tiered_access.rs`,
`mosaicfs-vfs/src/cache.rs`

Mounts a virtual filesystem tree constructed from CouchDB metadata. File
content is resolved through 5 tiers (`tiered_access.rs:51`):

- **Tier 1**: Local file on this node (direct read from `export_path`)
- **Tier 2**: Network mount (CIFS/NFS) — path translation via `network_mounts` config
- **Tier 3**: Cloud sync (iCloud/Google Drive) — local sync directory
- **Cache check**: Between tiers 3 and 4, checks local `FileCache` (SQLite-indexed,
  shard-based on disk)
- **Tier 4**: Remote agent HTTP fetch (`/api/agent/transfer/{file_id}`)
- **Tier 4b**: Replica failover (S3/B2 download, directory replica, agent replica)

The VFS caches fetched files locally (`cache.rs`) with LRU eviction, block-mode
support for large files, and digest verification.

### fs123 Capabilities

**Source:** `fs123-copy/` directory, commit state as of evaluation date.

**Warning from README:** "THIS IS AN UNFINISHED EXPERIMENTAL WORK IN PROGRESS"

fs123 is a FUSE-based read-only distributed filesystem that translates
filesystem operations into HTTP requests. Three crates:

| Crate | Purpose |
|-------|---------|
| fs123-core | Protocol types, HTTP client, netstring encoding |
| fs123-server | HTTP server with pluggable backend trait |
| fs123-client | FUSE client with application-level cache |

**Protocol** (v7.3): Single-letter HTTP endpoints —
`/a` (stat), `/d` (dir), `/f` (file read), `/l` (symlink), `/s` (statvfs),
`/x` (xattr), `/n` (stats), `/p` (passthrough).

**Server backends:**
- `FileBackend` — serves files from a local directory tree
- `DatabaseBackend` (feature-gated) — SQLite metadata + external content URLs

**Client caching:** Application-level stale-while-revalidate cache for
attributes, directories, and symlinks. File content relies on kernel page
cache (intentional design — avoids double-caching).

**ESTALE cookies:** Protocol-level mechanism for detecting stale file handles
when inodes are reused. Both client and server implement this.

---

## Evaluation: Where fs123 Could Replace Internal Code

### Scenario A: fs123 replaces only the agent file server

Each agent runs an fs123-server exporting its watched directories. Other nodes
use fs123-client to mount remote agents' trees.

**Today (agent file server):**
- Agent runs custom axum endpoint at `/internal/files/content`
  (`file_server.rs`, 114 lines)
- Server proxies to it via `proxy_to_agent()` (`files.rs:406-499`, ~95 lines)
- VFS fetches from it via `fetch_remote_file()` (`fuse_fs.rs:545-616`, ~70 lines)
- Bearer token auth, no caching at transport level

**Proposed:**
- Agent runs fs123-server exporting each watch_path
- Other nodes mount remote exports via fs123-client
- VFS reads remote files directly from the fs123 mount (a local path)
- Tiered access Tier 4 becomes a local read from a FUSE mount instead of HTTP

**What this eliminates:**
- `file_server.rs` (114 lines)
- `proxy_to_agent()` (95 lines)
- `fetch_remote_file()` (70 lines)
- Custom download token logic for agent-to-agent transfers

**What this gains:**
- Protocol-level caching with stale-while-revalidate (metadata)
- Kernel page cache for file content (already effective)
- ESTALE cookie consistency checking
- Clean HTTP-based transport with a well-defined protocol

### Scenario B: fs123 replaces both agent file server and VFS FUSE layer

A custom fs123 backend reads metadata from CouchDB (or redb, per changes/001)
and serves the virtual directory tree through the fs123 protocol. The entire
MosaicFS VFS is replaced by fs123-client.

**This does not work.** The MosaicFS VFS constructs its directory tree from:
- Virtual directory documents in CouchDB (`dir::*` prefixed)
- Label rules and mount entries
- Inherited readdir steps from parent directories
- File documents with metadata aggregated from multiple nodes

fs123's model is fundamentally different — it mirrors a single backend's
namespace. The directory entries it returns come from the backend's
`read_directory()` method. To make this work, you would need to write a custom
fs123 backend that reimplements all of `readdir.rs`, `reconciliation.rs`,
`inode.rs`, and `tiered_access.rs` behind the `Backend` trait. This would be
a rewrite of the VFS layer inside a foreign crate's abstraction, not a
simplification.

### Scenario C: fs123 as an additional transport tier

Add fs123 as a Tier 2.5 option between network mounts and remote HTTP fetch.
Nodes that share directories via fs123 get faster access than Tier 4 with
better caching, while the existing tier chain remains for fallback.

**This adds complexity without removing it.** The existing tiers would remain,
and fs123 would be yet another mechanism to configure and debug.

---

## Pros and Cons

### Pros of adopting fs123 (Scenario A)

1. **Eliminates custom file transport code.** The agent file server, proxy
   logic, and remote fetch logic (~280 lines) are replaced by a
   well-tested filesystem protocol.

2. **Better caching.** fs123-client's stale-while-revalidate cache for
   metadata means repeated `stat()` and `readdir()` calls against remote files
   are fast. Today, MosaicFS's Tier 4 path has no transport-level metadata
   caching.

3. **Consistency semantics.** ESTALE cookies and validators provide protocol-level
   guarantees that MosaicFS's HTTP fetching doesn't have. Today, a file could
   be modified on the source node between the CouchDB metadata lookup and the
   HTTP content fetch — fs123 detects this.

4. **Standard POSIX interface.** Once a remote directory is fs123-mounted,
   any process can read from it — not just MosaicFS. This could be useful
   for debugging or for tools that want to access remote files without going
   through the MosaicFS VFS.

5. **Separation of concerns.** File transport becomes fs123's problem. MosaicFS
   focuses on metadata federation, virtual tree construction, and policy
   (labels, replication). This aligns with the project's preference for
   explicit boundaries over implicit conventions.

6. **Backend abstraction enables future flexibility.** fs123-server's `Backend`
   trait could eventually support MosaicFS-aware backends (e.g., a backend that
   reads content from S3 replicas), but this is speculative future work.

### Cons of adopting fs123

1. **Additional FUSE mounts.** Each remote agent export becomes a separate
   FUSE mount on the local machine. With N agents each exporting M watch paths,
   that's up to N*M mounts. FUSE mount management (startup, health checking,
   reconnection) adds operational complexity.

2. **Loses Tier 1-3 fast paths.** MosaicFS's tiered access is designed so that
   local files, network mounts, and cloud sync directories are accessed without
   any network hop. If Tier 4 is replaced by fs123, the fast paths still need
   to exist. fs123 only replaces the remote-fetch path, not the local
   optimization paths. The net code reduction is limited to the Tier 4 transport.

3. **Loses Tier 4b replica failover.** When the owning node is offline,
   MosaicFS falls back to S3/B2 replicas or directory replicas. fs123 has no
   concept of fallback data sources — if the fs123-server is down, the mount
   returns errors. A separate mechanism would be needed to handle replica
   failover.

4. **No authentication.** fs123 has no built-in authentication or authorization.
   Today, the agent file server uses bearer tokens. With fs123, you would need
   either:
   - A reverse proxy with auth in front of fs123-server
   - Custom auth middleware added to the fs123-server codebase
   - Network-level isolation (only allow connections from known peers)

5. **Two FUSE layers.** If Scenario A is adopted, the local machine runs both
   the MosaicFS VFS FUSE mount and one or more fs123 FUSE mounts. Two FUSE
   layers for one use case (reading remote files) adds latency and debugging
   complexity.

6. **Deployment complexity.** Each agent would need to run both the existing
   MosaicFS agent and an fs123-server process. The current pod manifest
   (`deploy/mosaicfs.yaml`) would need an additional container or the
   fs123-server would need to be embedded in the agent binary.

7. **fs123 is experimental.** The README explicitly warns against using it with
   important data. Making MosaicFS dependent on it means taking on that risk.
   (Mitigated by: the same developer owns both projects.)

8. **macOS FileProvider direction.** Changes/001 plans to replace FUSE with
   macOS FileProvider on macOS. fs123-client is FUSE-based. On macOS, the
   two-layer approach (fs123 FUSE mount feeding a FileProvider) creates a
   FUSE dependency that changes/001 is trying to eliminate.

---

## Missing fs123 Features

Features that fs123 does not currently have that would be useful for MosaicFS
integration:

1. **Authentication/authorization.** No built-in auth mechanism. Required for
   any deployment where agents are on different networks or untrusted LANs.

2. **TLS support.** fs123-server uses plain HTTP (actix-web). Needed for
   encrypted transport between nodes.

3. **Multi-export support.** fs123-server exports a single root directory.
   An agent watching `/home/user/documents` and `/home/user/photos` would
   need either two fs123-server instances or a way to export multiple roots
   through a single server (using the SELECTOR mechanism, which exists in
   the protocol but is not implemented in the Rust server).

4. **Dynamic export configuration.** fs123-server's export root is set at
   startup via CLI. MosaicFS agents can update their watch_paths
   dynamically. fs123 would need a reload mechanism or a more flexible
   configuration model.

5. **Persistent content cache.** fs123-client relies on kernel page cache,
   which is lost on unmount/reboot. MosaicFS's `FileCache` persists on disk
   with LRU eviction, surviving restarts. For large files fetched over slow
   connections, losing the cache is expensive.

6. **Reconnection/retry.** fs123-client's HTTP client (ureq, blocking) has
   basic error handling but no automatic reconnection to a server that was
   temporarily unavailable. In a peer-to-peer mesh, nodes come and go.

7. **Service discovery.** fs123-client needs a host/port at startup. In
   MosaicFS's mesh, agents register their endpoints in CouchDB. Some glue
   is needed to translate CouchDB node documents into fs123 mount
   configurations.

8. **Content verification.** fs123 does not verify content integrity (no
   checksums in the protocol). MosaicFS's remote fetch includes digest
   verification (`parse_digest_header` in `fuse_fs.rs`).

---

## Recommendation

**Scenario A (replace agent file server only) is architecturally sound but
premature given the current project state.**

The concept of using fs123 as the file transport layer between nodes is clean
and aligns with the project's principles (explicit boundaries, boring
technology, separation of concerns). However:

- Changes/001 introduces a major architectural shift (Loco, redb, FileProvider,
  unified binary). Adding fs123 integration on top of that creates two
  simultaneous moving parts, violating the "one moving part at a time" principle.

- The missing features list (auth, TLS, multi-export, reconnection) represents
  significant work in the fs123 codebase before it could serve as a reliable
  transport in MosaicFS.

- The actual code being replaced (~280 lines of agent file server + proxy logic)
  is straightforward and works. The complexity in MosaicFS's file access is in
  the tiered access logic and metadata-driven virtual tree, neither of which
  fs123 touches.

**If pursued, the right time is after changes/001 stabilizes**, and the right
scope is:

1. Add auth + TLS to fs123-server (prerequisite, done in the fs123 repo)
2. Add multi-export via SELECTOR (prerequisite)
3. Embed fs123-server in the unified MosaicFS binary as an optional component
4. Use fs123 mounts as the Tier 4 transport, keeping Tiers 1-3 and 4b intact
5. Deprecate the custom agent file server

This is a well-defined, bounded change that can be its own `docs/changes/`
entry when the time comes.
