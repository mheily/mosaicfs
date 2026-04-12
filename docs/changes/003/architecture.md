# Architecture Change 003: No MosaicFS Transport Layer & Lazy Path Resolution

This change formalizes two principles for inter-node data access and removes the
code that violates them. It is part of the settled architectural direction: each
node is a peer with its own view of which filesystems it can reach via the OS,
and MosaicFS relies on the OS rather than building its own transport.

## Current State Summary

_Verified against the tree at commit 6266c02._

**Workspace:** four crates — `mosaicfs-common` (1.6k lines), `mosaicfs-agent`
(3.7k lines), `mosaicfs-server` (7.2k lines), `mosaicfs-vfs` (3.5k lines). Two
binaries (`mosaicfs-server`, `mosaicfs-agent`) built into one container image.

**Document model** (`mosaicfs-common/src/documents.rs`):

- `FileDocument.source: FileSource { node_id, export_path, export_parent }` —
  every file records the node that owns it and the absolute path on that node.
- `NodeDocument.storage: Vec<StorageEntry>` — each entry has
  `filesystem_id, mount_point, fs_type, device, capacity_bytes, used_bytes`
  and lists the local filesystems on the node.
- `NodeDocument.network_mounts: Vec<NetworkMount>` — each entry has
  `mount_id, remote_node_id, remote_base_export_path, local_mount_path,
  mount_type, priority` and lists remote filesystems that this node has mounted
  (CIFS, NFS, iCloud local, Google Drive local, etc.).
- `NodeDocument.transfer: Option<TransferConfig { endpoint, protocol }>` —
  advertises the node's own file-transfer HTTP endpoint.
- No top-level "filesystem document" exists today. Filesystem identity is
  implicit in `storage[].filesystem_id` and `network_mounts[].mount_id`.

**VFS access path** (`mosaicfs-vfs/src/tiered_access.rs`):

- **Tier 1** — local file on this node; direct read from `source.export_path`
  after a watch-path containment check.
- **Tier 2** — network mount (CIFS/NFS); path is translated via the matching
  `NetworkMount` entry and opened as a local path.
- **Tier 3** — local cloud-sync directory (iCloud/gDrive); same translation,
  plus an iCloud eviction check.
- **Tier 4** — remote agent HTTP fetch; MosaicFS issues an HMAC-signed
  `GET /api/agent/transfer/{file_id}` to the owning node's transfer endpoint,
  streams to a cache dir, and serves from cache.
- **Tier 4b** — replica failover when Tier 4 fails. For `agent` backend this is
  another HTTP fetch; for `s3`/`b2` it downloads from object storage; for
  `directory` it tries a locally mounted replica path.
- **Tier 5** — plugin materialize on the owning node, invoked during a Tier 4
  request (described in `docs/architecture/07-vfs-access.md`).

**Transport endpoints:**

- `mosaicfs-agent/src/file_server.rs` — agent-side `/internal/files/content`
  HTTP server that streams file bytes given a path and bearer token.
- `mosaicfs-server/src/handlers/files.rs::proxy_to_agent` — server-side path
  that proxies `/api/files/{id}/download` through to the owning agent's file
  server when the file is not present on the server's own filesystem.

**Readdir** (`mosaicfs-vfs/src/readdir.rs`): resolves directory listings by
querying CouchDB for file metadata (`find` on the `source.node_id` /
`source.export_parent` index). It does **not** stat remote filesystem paths
during listing; traversal of actual filesystem paths happens only inside
`tiered_access::resolve_file` when a client opens a file.

## Goal

Make it a settled invariant that MosaicFS carries no file bytes over its own
network; all inter-node data access is the OS's responsibility via mounts that
each node manages and publishes. Remove the transport code that currently
exists (Tier 4, Tier 4b/agent, and the agent-side file server) and formalize
the per-filesystem availability map that other nodes use to decide whether a
file is reachable.

## Changes

### Change A — Remove the MosaicFS transport layer

**Today:** The VFS falls through to an HTTP fetch (Tier 4 /
`AccessResult::NeedsFetch`) when no local or mounted path is available. The
owning agent exposes a bearer-auth HTTP file server (`file_server.rs`,
port 8444 in the pod manifest) that serves bytes from arbitrary paths under the
agent's watch roots. The server proxies cross-node file-content requests via
`proxy_to_agent`.

**Proposed:** Remove all of the above. The VFS access chain ends at Tier 3
(mounted paths). When no node-local path exists, `resolve_file` returns
`AccessResult::NotAccessible` and the FUSE `open` returns `ENOENT`/`EIO`.
`mosaicfs-agent` no longer opens a listening socket for file bytes. The server
route `/api/files/{id}/download` either serves from a locally reachable path
(including paths reached via a network mount that the server itself has
mounted) or returns `NOT_FOUND` with a reason.

**Justification:** Maintaining a transport server duplicates the work of
NFS/SMB and places MosaicFS on the critical path for every cross-node read. OS
transports are battle-tested, integrate with existing credentials
(Kerberos/AD/etc.), and are already how the user mounts remote storage.
Removing the transport also eliminates a class of security surface — the
agent's bearer-auth HTTP server, its HMAC download-token logic, and the
watch-path containment check that exists solely to stop the transport from
leaking files outside configured roots.

### Change B — Introduce a per-filesystem availability map

**Today:** Filesystem identity is scattered. Local filesystems are described
by `StorageEntry.filesystem_id` on the owning node's doc; remote access is
described by `NetworkMount.mount_id` on each mounting node's doc. There is no
single document that represents "filesystem Foo" and lists every node that can
reach it. To answer "can node B see Foo?" we scan `node::B`'s `network_mounts`
and pattern-match against `remote_node_id` + `remote_base_export_path`.

**Proposed:** Add a `FilesystemDocument` with id
`filesystem::<fs-id>` that captures the stable identity of each filesystem
exported into the MosaicFS namespace:

```rust
pub struct FilesystemDocument {
    pub doc_type: FilesystemType,           // "filesystem"
    pub filesystem_id: String,              // stable id
    pub friendly_name: String,
    pub owning_node_id: String,             // authoritative source
    pub export_root: String,                // absolute path on the owning node
    pub availability: Vec<NodeAvailability>,
    pub created_at: DateTime<Utc>,
}

pub struct NodeAvailability {
    pub node_id: String,
    pub local_mount_path: String,
    pub mount_type: String,                 // "local", "nfs", "cifs", "icloud_local", …
    pub last_seen: DateTime<Utc>,
}
```

Each node updates its own `NodeAvailability` row via CouchDB's
single-field-update pattern (read-modify-write on the filesystem doc, retry on
conflict). `NetworkMount` and `StorageEntry` stay — they remain the on-node
authoritative record of what this node has mounted — but each entry gains a
`filesystem_id` that links to the shared document.

**Justification:** Principle 1 ("per-node availability map") calls for an
explicit structure that any node can consult to decide whether to attempt
access. The existing per-node model works for Tier 2 translation but does not
surface "which nodes can see filesystem X" without a scan. The shared doc
turns that into a single lookup and gives the UI a natural place to visualize
filesystem reachability.

### Change C — Enforce lazy path resolution as an invariant

**Today:** Readdir is already lazy — `readdir.rs` reads metadata from CouchDB
and never touches remote filesystem paths. `tiered_access.rs` touches paths
only when a client has explicitly opened a file. No proactive stat or
health-check of remote mounts exists in the VFS code path I reviewed. The
invariant is not documented, and any future code could break it (e.g. a
"refresh mount state" tick that stats every `local_mount_path`).

**Proposed:** Write the invariant down. Document in
`docs/architecture/07-vfs-access.md` that the VFS must never proactively stat,
enumerate, or health-check paths belonging to remote filesystems, and reference
the rule from `NetworkMount` / `FilesystemDocument` type docstrings. No code
helper or runtime guard — the rule is enforced by review.

**Justification:** Principle 2 is a correctness constraint that today holds by
accident of the current code shape. A well-intentioned future change
(background mount health check, proactive mount warm-up) could reintroduce
hangs across every unavailable mount. Writing the rule down is cheap and gives
reviewers something concrete to point at.

## Implementation Phases

Phases are organized by topical focus, not by deployability. The tree may be
broken or have failing tests between phases; only the state after Phase 4 must
be correct.

**Phase 1 — Remove the transport.**
Delete:

- `mosaicfs-agent/src/file_server.rs` and the port 8444 listener in
  `deploy/mosaicfs.yaml`.
- `AccessResult::NeedsFetch`, `FetchInfo`, and the Tier 4 branch of
  `tiered_access::resolve_file`.
- `fetch_remote_file` and the `NeedsFetch` arm of `Filesystem::open` in
  `mosaicfs-vfs/src/fuse_fs.rs`.
- The `"agent"` branch in `resolve_from_replica` and the
  `get_agent_replica_endpoint` stub.
- `proxy_to_agent` in `mosaicfs-server/src/handlers/files.rs`; collapse
  `serve_file_content` to the local-path branch.
- HMAC download-token logic in `files.rs` (dead without the transport).
- `TransferConfig` and the `transfer` field on `NodeDocument`.

The `s3`/`b2`/`directory` branches of Tier 4b stay. Watch-path containment in
Tier 1 stays.

**Phase 2 — Introduce `FilesystemDocument`.**
Add the document type in `mosaicfs-common/src/documents.rs` (with ts-rs
bindings) and the corresponding CouchDB id convention. Add `filesystem_id` to
`NetworkMount` and confirm it on `StorageEntry`. No publishers or readers yet
— this phase is purely the schema.

**Phase 3 — Publish and consume availability.**
Agent-side: on every heartbeat, upsert the `filesystem::<fs-id>` doc for each
local `StorageEntry` (this node is the owner), and update the matching
`NodeAvailability` row for each reachable `NetworkMount`. Use read-modify-write
with conflict retry on the shared doc. VFS-side: replace the per-node
`network_mounts` scan in Tier 2/3 with a lookup against
`FilesystemDocument.availability`.

**Phase 4 — Document the lazy-resolution invariant.**
Update `docs/architecture/07-vfs-access.md` to state the invariant, remove the
Tier 4 / Tier 5-cross-node sections that no longer apply, and reference the
rule from doc-comments on `NetworkMount` and `FilesystemDocument`. Touch
`docs/architecture/20-open-questions.md` if any open questions are now
resolved or reframed.

**Phase dependencies:**

- Phase 3 requires Phase 2 (the doc must exist before publishers/readers use it).
- Phase 1 is independent and can land first, last, or in parallel; it leaves
  the tree with no fallback for unreachable files, which is the intended end
  state once Phase 3 also lands.
- Phase 4 can run any time; it only touches docs.

## What Does Not Change

- **CouchDB federation.** Metadata sync, conflict handling, `_changes` feeds,
  and the existing `db` routes are unaffected.
- **Local file access (Tier 1).** Same code path, same containment check.
- **Network mount access (Tier 2/3).** The translation logic in
  `translate_network_path` and the iCloud eviction check stay; they just look
  up availability via the new document.
- **Virtual directory and mount-entry model.** `VirtualDirectoryDocument`,
  `MountEntry`, `MountSource`, step pipeline, `ConflictPolicy`, and the
  readdir evaluator are untouched.
- **Replication to S3/B2/directory targets.** Offsite replication is a separate
  concern from inter-node data transport; `StorageBackendDocument`,
  `ReplicaDocument`, `ReplicationRuleDocument`, and `mosaicfs-agent/src/
  replication*.rs` are unaffected.
- **User-facing REST API surface.** The ~92 routes under `/api/*` keep their
  shapes. Only the internal behavior of `/api/files/{id}/download` changes (no
  more cross-node proxy) and the agent's internal port 8444 goes away.
- **Loco/HTMX migration (change 001), fs123 evaluation (change 002).** Parallel
  concerns; this change does not advance or reverse either.
- **Authentication, credentials, JWT handling.** Aside from the dead-on-arrival
  download-token functions in `files.rs`, the auth stack is unchanged.
- **Deployment model.** Still one container image, still one pod with CouchDB
  alongside. The pod manifest loses the agent's port 8444 mapping; nothing is
  added.

## Deferred

- **Write-side semantics.** Today the VFS is read-only. Whether writes flow
  through the OS mount (and how conflicts resolve) is out of scope for this
  change.
- **Automated mount discovery and wiring.** Nodes still publish mounts the user
  has configured; MosaicFS does not mount filesystems on the user's behalf.
- **Credential delegation for OS mounts.** Kerberos tickets, SMB creds, iCloud
  sign-in state, etc. remain the user's responsibility to provision; MosaicFS
  reports what is reachable, not how to reach it.
- **Per-file availability (vs. per-filesystem).** A filesystem being reachable
  does not guarantee every file inside it is readable (permissions, ACLs). The
  OS continues to handle this per-open; no separate modeling is added.
- **Tier 5 (plugin materialize) across nodes.** With transport gone, a plugin
  on node A cannot materialize a file on behalf of node B's open. If this
  matters later, it becomes its own change — likely a scheduled plugin run
  that lands bytes into a shared filesystem rather than a synchronous fetch.
- **Replacing replica failover with OS semantics.** Tier 4b's `s3`/`b2`/
  `directory` paths stay as-is; unifying those with "just another mount" would
  be a larger rework and isn't needed now.
- **Replacing the agent/server split.** Unifying the two binaries is the
  stated direction (project decisions) but is a separate change. This one
  shrinks the agent's surface in a way that makes the eventual unification
  easier.
- **Soft-fail UI for unreachable filesystems.** Showing "this file lives on
  node B, which node A cannot currently reach" in the file browser is a UI
  concern that can follow once the availability map exists.
