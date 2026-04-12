# Change 003 — Design Notes

Implementation specifics for the change described in `architecture.md`. Read
that document first; this one assumes its vocabulary and decisions.

## Reading guide

Each phase below lists the exact files to edit, what to delete, what to add,
and what shape the result should have. Phase order is topical, not
deployable — the tree may be broken between phases. A final "Verification"
section at the end lists the commands to run after **all** phases land.

The crates touched: `mosaicfs-common`, `mosaicfs-agent`, `mosaicfs-server`,
`mosaicfs-vfs`. The `web/` frontend is regenerated, not hand-edited (ts-rs
emits the bindings from Rust types).

A note before starting: nothing currently calls `mosaicfs_vfs::fuse_fs::mount`
from a binary. The VFS code in `mosaicfs-vfs` is exercised only by its own
tests today. That means changes to `FuseConfig` and `resolve_file` signatures
have **no external callers to update** outside the crate's tests.

---

## Phase 1 — Remove the transport

### Delete files

- `mosaicfs-agent/src/file_server.rs` — entire file.

### `mosaicfs-agent/src/main.rs`

- Remove the `mod file_server;` declaration (line 5).
- Remove `const FILE_SERVER_PORT: u16 = 8444;` (line 28).
- Remove the file-server-host computation block (lines 64-68: `file_server_host`,
  `file_server_url`).
- Remove the `agent_token` line (71) and the `tokio::spawn` block that calls
  `file_server::start` (74-79).
- Update the `register_node` call (line 87) to drop the `&file_server_url` and
  `&agent_token` arguments. The new signature is in the `node.rs` section
  below.

### `mosaicfs-agent/src/node.rs`

- Drop the `file_server_url: &str` and `agent_token: &str` parameters from
  `register_node`.
- In the existing-doc branch, remove the assignments to
  `existing["file_server_url"]` and `existing["agent_token"]` (lines 39-40).
- In the new-doc branch, remove `"file_server_url"` and `"agent_token"` from
  the JSON literal (lines 55-56).
- (The `storage` block stays — Phase 3 extends it.)

### `mosaicfs-vfs/src/tiered_access.rs`

- Delete `AccessResult::NeedsFetch(FetchInfo)` and the entire `FetchInfo`
  struct.
- In `resolve_file`, delete the entire Tier 4 block (everything from
  `// Tier 4: Remote agent fetch` through the matching `}` — currently
  lines 160-186). Replace it with a direct call to `resolve_from_replica` so
  Tier 4b becomes the immediate fallback when no node-local path satisfies
  the open. The replaced tail of `resolve_file` looks like:

  ```rust
      // No node-local access path; fall through to replica failover.
      resolve_from_replica(file, db, watch_paths, network_mounts, cache).await
  }
  ```

- In `resolve_from_replica`, delete the entire `"agent" => { … }` arm
  (lines 260-281) and the `get_agent_replica_endpoint` helper
  (lines 497-503). The remaining backends (`directory`, `s3`, `b2`, `_`) are
  unchanged.
- Delete `get_transfer_endpoint` (lines 577-591).
- Keep `NetworkMountInfo` for now — Phase 3 replaces it.

### `mosaicfs-vfs/src/fuse_fs.rs`

- Change the `use crate::tiered_access::{...}` import to drop `AccessResult`'s
  `NeedsFetch` variant references; the import line itself stays.
- In `Filesystem::open`, replace the `match access_result { … }` block with a
  two-arm match:

  ```rust
  match access_result {
      AccessResult::LocalPath(path) => {
          let fh = self.alloc_fh();
          self.open_files.lock().unwrap().insert(fh, (file.clone(), path));
          reply.opened(fh, 0);
      }
      AccessResult::NotAccessible(msg) => {
          warn!(file_id = %file.file_id, reason = %msg, "File not accessible");
          reply.error(libc::EIO);
      }
  }
  ```

  (`ENOENT` was used in one branch and `EIO` in the other; standardize on
  `EIO` since the file *exists* in the namespace, just isn't reachable.)

- Delete `fetch_remote_file` (lines 544-616) and `parse_digest_header`
  (lines 618-625).
- Drop the `base64` and `sha2` imports if they become unused after the
  deletions (compiler will tell you).

### `mosaicfs-server/src/handlers/files.rs`

- Delete the entire "Download tokens" section (lines 19-67):
  `DOWNLOAD_TOKEN_EXPIRY_SECS`, `issue_download_token`,
  `validate_download_token`, `constant_time_eq`.
- Delete `get_file_token` (lines 194-225), `download_file` (lines 229-246),
  `DownloadTokenQuery` (lines 248-251).
- Delete `proxy_to_agent` (lines 406-500).
- In `serve_file_content`, replace the `if !path.exists() { return
  proxy_to_agent(...) }` block (lines 288-291) with a direct
  `NOT_FOUND` response:

  ```rust
  if !path.exists() {
      return (StatusCode::NOT_FOUND, Json(error_json(
          "File not present on this node and inter-node transport has been removed"
      ))).into_response();
  }
  ```

- Delete the `base64_encode` helper (lines 517-520) — dead.
- Drop now-unused imports (`hmac`, `sha2::{Digest, Sha256}`, `Hmac`,
  `urlencoding` if no longer used, `chrono::Utc` if no longer used).

### `mosaicfs-server/src/routes.rs`

- Delete the `/api/files/{file_id}/token` route (line 56).
- Delete the `/api/files/{file_id}/download` route and its surrounding
  comment (lines 142-143).

### `mosaicfs-common/src/documents.rs`

- Delete `TransferConfig` (lines 240-245) and remove the `transfer:
  Option<TransferConfig>` field from `NodeDocument` (line 215).
- Update the `test_node_document` test (line 770) to drop the `transfer: None`
  field from the literal.

### `deploy/mosaicfs.yaml`

- Remove the `- containerPort: 8444` line (line 177) and any surrounding
  port-mapping/comment specific to the agent file server.

### `Cargo.toml` (workspace and per-crate)

- Audit `mosaicfs-server` and `mosaicfs-vfs` `Cargo.toml` for now-unused
  dependencies after the deletions (`hmac`, `base64`, possibly
  `urlencoding` in the server). Remove unused entries; leave anything still
  imported elsewhere.

---

## Phase 2 — Introduce `FilesystemDocument`

### `mosaicfs-common/src/documents.rs`

Add the following types after the `// ── Node Document ──` block. Mirror the
existing patterns: `serde` rename for `type`, `ts-rs` `#[ts(export)]`, a
`doc_id` helper.

```rust
// ── Filesystem Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct FilesystemDocument {
    #[serde(rename = "type")]
    pub doc_type: FilesystemType,
    pub filesystem_id: String,
    pub friendly_name: String,
    pub owning_node_id: String,
    pub export_root: String,
    #[serde(default)]
    pub availability: Vec<NodeAvailability>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub enum FilesystemType {
    #[serde(rename = "filesystem")]
    Filesystem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct NodeAvailability {
    pub node_id: String,
    pub local_mount_path: String,
    /// "local" when this is the owning node, otherwise the OS mount type
    /// (e.g. "nfs", "cifs", "icloud_local", "gdrive_local").
    pub mount_type: String,
    pub last_seen: DateTime<Utc>,
}

impl FilesystemDocument {
    pub fn doc_id(filesystem_id: &str) -> String {
        format!("filesystem::{}", filesystem_id)
    }
}
```

Add a round-trip test next to the existing ones:

```rust
#[test]
fn test_filesystem_document() {
    let doc = FilesystemDocument {
        doc_type: FilesystemType::Filesystem,
        filesystem_id: "fs-laptop-home".to_string(),
        friendly_name: "Laptop home".to_string(),
        owning_node_id: "node-laptop".to_string(),
        export_root: "/home/user".to_string(),
        availability: vec![NodeAvailability {
            node_id: "node-laptop".to_string(),
            local_mount_path: "/home/user".to_string(),
            mount_type: "local".to_string(),
            last_seen: now(),
        }],
        created_at: now(),
    };
    round_trip_couch("filesystem::fs-laptop-home", doc);
}
```

### Schema additions to existing types

Add `pub filesystem_id: String` to `NetworkMount` (after `mount_id`). Update
the existing `NetworkMount` test if one exists, and the `network_mounts`
field in `test_node_document` if it's populated.

`StorageEntry.filesystem_id` already exists and serves as the local
filesystem id.

### Filesystem ID derivation

The owning node assigns the id. Use a stable, human-readable format:

```
filesystem_id = format!("{}::{}", owning_node_id,
    sanitize(export_root))
```

where `sanitize` lower-cases and replaces non-alphanumerics with `-`,
collapsing runs (e.g. `/home/user` → `home-user`). Add a small helper in
`mosaicfs-common`:

```rust
pub fn derive_filesystem_id(owning_node_id: &str, export_root: &str) -> String {
    let s: String = export_root
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let collapsed: String = s.split('-').filter(|p| !p.is_empty())
        .collect::<Vec<_>>().join("-");
    format!("{}::{}", owning_node_id, collapsed)
}
```

For `NetworkMount`, the `filesystem_id` field is set by the mounting node to
match what the owning node would publish — the convention is the same string,
derived from `remote_node_id` + `remote_base_export_path`.

---

## Phase 3 — Publish and consume availability

### Agent: publishing (`mosaicfs-agent/src/node.rs`)

Add a new function `publish_filesystem_availability` called from
`register_node` and `heartbeat`:

```rust
pub async fn publish_filesystem_availability(
    db: &CouchClient,
    node_id: &str,
    storage: &[StorageEntry],
    network_mounts: &[NetworkMount],
) -> anyhow::Result<()> {
    // For each local StorageEntry: this node OWNS the filesystem.
    for entry in storage {
        upsert_filesystem(db, node_id, entry.filesystem_id.clone(),
            /* owning_node_id */ node_id,
            /* export_root */ &entry.mount_point,
            /* mount_type */ "local",
            /* local_mount_path */ &entry.mount_point,
        ).await?;
    }
    // For each NetworkMount: this node is a CONSUMER.
    for nm in network_mounts {
        upsert_filesystem(db, node_id, nm.filesystem_id.clone(),
            /* owning_node_id */ &nm.remote_node_id,
            /* export_root */ &nm.remote_base_export_path,
            /* mount_type */ &nm.mount_type,
            /* local_mount_path */ &nm.local_mount_path,
        ).await?;
    }
    Ok(())
}
```

`upsert_filesystem` performs read-modify-write with conflict retry (max 3
attempts) on `filesystem::<id>`:

1. `db.get_document(&doc_id)` — if NotFound, create a fresh doc with
   `owning_node_id`, `export_root`, `created_at = now`, empty availability.
2. Replace any existing `availability` row with `node_id == self.node_id`,
   set `last_seen = now`. If absent, push a new row.
3. `db.put_document(&doc_id, &doc)`. On 409 conflict, re-read and retry
   (up to 3 times). On final failure, log a warning and continue — missing
   availability is recoverable on the next heartbeat.

Crucially: a consumer-only update **must not overwrite** `owning_node_id` or
`export_root` if the doc already exists with different values; instead, log a
warning and skip the upsert. This prevents a misconfigured network mount from
corrupting the owning node's record.

Call sites:

- `register_node` — after the `db.put_document(&doc_id, &doc).await?` at
  line 64, parse the storage and network_mounts back out (or pass them
  through from `main.rs`) and call `publish_filesystem_availability`.
- `heartbeat` — after writing the heartbeat update, do the same.
- `set_offline` — clear this node's row from each filesystem doc's
  `availability` (same upsert pattern, but removing the row instead of
  refreshing it).

### Agent: where do `network_mounts` come from?

Today the agent never populates `network_mounts` on the node doc — the field
exists in the schema but no code writes it. This change does **not** add
discovery of network mounts; that is deferred (see architecture.md
"Deferred"). For Phase 3, treat the agent's `network_mounts` list as
whatever the user has placed there manually (or via a follow-on UI change).
The publisher loops over whatever is present.

### VFS: consuming (`mosaicfs-vfs/src/tiered_access.rs` & `fuse_fs.rs`)

Replace `NetworkMountInfo` with a richer cache that mirrors the published
documents:

```rust
#[derive(Debug, Clone)]
pub struct FilesystemView {
    pub filesystem_id: String,
    pub owning_node_id: String,
    pub export_root: String,
    pub local_mount_path: Option<String>,  // Some when this node has access
    pub mount_type: String,                 // "local" if this node is owner
}
```

Add a loader on the VFS side (in a new `filesystem_view` module under
`mosaicfs-vfs/src/`) that:

- Queries `filesystem::*` from CouchDB (using existing
  `all_docs_by_prefix`).
- Parses each into a `FilesystemDocument`, then projects into
  `FilesystemView` using the local `node_id` to extract the matching
  availability row.

Update `FuseConfig`:

```rust
pub struct FuseConfig {
    pub node_id: String,
    pub watch_paths: Vec<PathBuf>,
    pub filesystems: Vec<FilesystemView>,   // replaces network_mounts
    pub mount_point: PathBuf,
    pub cache_dir: PathBuf,
    pub cache_cap: u64,
    pub min_free_space: u64,
}
```

Refresh policy: load on `MosaicFs::new`; refresh whenever the open path
returns `NotAccessible` (lazy reload — the user already paid for a failed
open, so a fresh CouchDB query is cheap). A periodic background refresher
is **not** added in this phase (matches the lazy-resolution principle).

Rewrite `resolve_file`:

1. Tier 1 unchanged — local file on owning node, watch-path containment
   check.
2. Tier 2/3: find the `FilesystemView` whose `owning_node_id ==
   file.source_node_id` and whose `export_root` is a prefix of
   `file.source_export_path`. If `local_mount_path.is_some()`, translate the
   path (existing `translate_network_path` logic) and try to open. The
   `mount_type` field decides whether to do the iCloud eviction check.
3. No node-local access → `resolve_from_replica` (unchanged tail from
   Phase 1).

Drop the `network_mounts: &[NetworkMountInfo]` parameter from
`resolve_file`, `resolve_from_replica`, `resolve_from_replica_for_open`,
and `fetch_remote_file`'s callers (the function itself is gone). Replace
with `filesystems: &[FilesystemView]`.

Update the `tiered_access` tests:

- Drop tests that exercised Tier 4 / `NeedsFetch` (deleted in Phase 1).
- Replace `translate_network_path` callers with a `FilesystemView`-driven
  test demonstrating the lookup-then-translate path.

---

## Phase 4 — Documentation

### `docs/architecture/07-vfs-access.md`

Rewrite to reflect three tiers + replica failover:

- Drop "Tier 4 — Remote fetch" entirely.
- Drop the "agent" branch description in "Tier 4b — Replica failover".
- Drop "Tier 5 — Plugin materialize" (cross-node materialize is gone with
  the transport; if a plugin still runs locally and writes into a watched
  path, it appears as a normal local file — no special tier needed).
- Drop the `GET /api/agent/transfer/{file_id}` block at the end.
- Add a new top-level paragraph stating the lazy-resolution invariant:

  > **Lazy path resolution.** The VFS must never proactively stat,
  > enumerate, or health-check paths belonging to remote filesystems. A
  > remote mount is touched only when a user explicitly opens a file under
  > it. Eager traversal risks hanging NFS responses for the duration of the
  > OS timeout on every unavailable mount in the cluster. Reviewers should
  > reject any change that adds background probes against
  > `NetworkMount.local_mount_path` or `NodeAvailability.local_mount_path`.

- Replace references to "owning node's `transfer.endpoint`" with a pointer
  to the new `FilesystemDocument` lookup.

### `docs/architecture/06-document-schemas.md`

Add a section for `FilesystemDocument` with field descriptions matching the
new struct. Document the id convention (`filesystem::<owning_node>::<sanitized-path>`).
Note that `NetworkMount.filesystem_id` and `StorageEntry.filesystem_id`
both reference this doc.

### `docs/architecture/20-open-questions.md`

Remove or update entries that are now answered by this change (search for
`transport`, `tier 4`, `agent fetch`, `download token`).

### Doc-comments in code

Add a doc-comment on `FilesystemDocument`, `NetworkMount`, and the new
`FilesystemView` that references the lazy-resolution invariant in
`07-vfs-access.md`.

---

## Verification (after all phases)

Run from the repo root:

```sh
# 1. Compile and test the workspace
cargo test --workspace

# 2. Regenerate ts-rs bindings (FilesystemDocument, NodeAvailability, etc.
#    will appear under web/src/types/generated/, and TransferConfig.ts and
#    related dead bindings should disappear)
cargo test --workspace ts_export

# 3. Confirm the agent binary no longer references port 8444
! grep -r "8444" mosaicfs-agent/ deploy/

# 4. Confirm no surviving references to deleted symbols
! grep -r "TransferConfig\|file_server\|fetch_remote_file\|NeedsFetch\|FetchInfo\|proxy_to_agent\|download_token\|get_file_token" \
    mosaicfs-agent/src mosaicfs-server/src mosaicfs-vfs/src mosaicfs-common/src

# 5. Build and redeploy per CLAUDE.md
make mosaicfs-image && podman kube play --replace deploy/mosaicfs.yaml

# 6. After deploy, inspect CouchDB to confirm filesystem docs appear
curl -u "$COUCHDB_USER:$COUCHDB_PASSWORD" \
    "$COUCHDB_URL/mosaicfs/_all_docs?startkey=\"filesystem::\"&endkey=\"filesystem::\\ufff0\"&include_docs=true" \
    | jq '.rows[].doc | {filesystem_id, owning_node_id, availability}'
```

A clean run should show one `filesystem::*` doc per `StorageEntry` declared
on the agent, each with a single `availability` row pointing at the local
mount. No `transfer` field on node docs. No `file_server_url` or
`agent_token` field on node docs. The pod should expose only port 5984
(CouchDB) and 8443 (server); 8444 should be gone.
