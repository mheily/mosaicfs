# Phase 8 — Backup and Restore Implementation Plan

## Context

Phase 8 adds the ability to export the MosaicFS control plane state as a JSON backup file and restore it into a fresh instance. Two backup modes exist: **minimal** (user-generated configuration only, <10 MB) and **full** (complete CouchDB snapshot). A **developer mode** flag enables a destructive wipe endpoint for rapid testing. The web UI already has backup download buttons in the About tab (`SettingsPage.tsx:341-405`); this phase implements the server-side handlers and adds restore UI.

**Milestone:** Take a minimal backup, destroy the Compose stack, recreate it, restore the backup, see virtual directories and replication configs reappear. Agents reconnect and re-crawl.

**Current state:**
- `GET /api/system/backup` and `POST /api/system/restore` are `stub_501` in `routes.rs:114-115`
- `GET /api/health` and `GET /api/system/info` are also `stub_501` (lines 112-113) — implement as part of this phase since the About tab already calls `/api/system/info`
- The About tab has working backup buttons that call `window.open('/api/system/backup?type=...')` but the endpoint returns 501
- `AppState` already has CouchDB credentials stored (`couchdb_url`, `couchdb_user`, `couchdb_password`)
- `CouchClient` has `all_docs_by_prefix()`, `bulk_docs()`, `get_document()`, `delete_document()`, `find()` — all needed primitives exist
- No `all_docs()` (without prefix) method exists — will need one for full backup and document counting

---

## Step 1: Add CouchDB helper methods

**File:** `mosaicfs-server/src/couchdb.rs`

### 1a: `all_docs()` — fetch all documents

Add a method to fetch all documents (with `include_docs=true`), needed for full backup and document counting.

```rust
pub async fn all_docs(&self, include_docs: bool) -> Result<AllDocsResponse, CouchError>
```

Implementation: `GET /{db}/_all_docs?include_docs={true|false}`. Filter out design documents (IDs starting with `_design/`) in the caller, not here.

### 1b: `db_info()` — get database metadata

Add a method to fetch database info (document count), needed for the empty-DB check before restore.

```rust
pub async fn db_info(&self) -> Result<serde_json::Value, CouchError>
```

Implementation: `GET /{db}/` — returns JSON with `doc_count`, `doc_del_count`, `update_seq`, etc.

### 1c: `delete_db()` and `ensure_db()` already exist

`ensure_db()` exists. Add `delete_db()` for the developer-mode wipe (drop + recreate is cleaner than deleting every document):

```rust
pub async fn delete_db(&self) -> Result<(), CouchError>
```

Implementation: `DELETE /{db}/`.

---

## Step 2: Add `--developer-mode` flag to AppState

**Files:** `mosaicfs-server/src/state.rs`, `mosaicfs-server/src/main.rs`

### 2a: Add field to `AppState`

Add `pub developer_mode: bool` to the `AppState` struct. Update `AppState::new()` to accept it as a parameter.

### 2b: Parse CLI flag in `main.rs`

Before the existing `if std::env::args().nth(1).as_deref() == Some("bootstrap")` block, parse a `--developer-mode` flag:

```rust
let developer_mode = std::env::args().any(|a| a == "--developer-mode");
if developer_mode {
    tracing::warn!("Developer mode enabled — DELETE /api/system/data is active");
}
```

Pass `developer_mode` to `AppState::new()`.

---

## Step 3: Implement system handlers

**New file:** `mosaicfs-server/src/handlers/system.rs`

Follow the existing handler patterns from `nodes.rs` and `notifications.rs`.

### 3a: `health` — `GET /api/health`

Simple health check. Verify CouchDB is reachable by calling `db_info()`.

```rust
pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse
```

Response:
```json
{ "status": "ok", "couchdb": "ok" }
```

On CouchDB failure:
```json
// HTTP 503
{ "status": "degraded", "couchdb": "unreachable" }
```

### 3b: `system_info` — `GET /api/system/info`

Return instance metadata. The About tab already calls this endpoint and expects `version`, `uptime`, `pouchdb_doc_count`, `pouchdb_update_seq`.

```rust
pub async fn system_info(State(state): State<Arc<AppState>>) -> impl IntoResponse
```

Implementation:
- Call `state.db.db_info()` to get CouchDB doc count and update sequence
- Use a static `Instant` (via `once_cell::sync::Lazy` or `std::sync::OnceLock`) initialized at startup for uptime
- Hardcode version from `env!("CARGO_PKG_VERSION")`

Response:
```json
{
  "version": "0.1.0",
  "uptime": "2h 34m",
  "pouchdb_doc_count": 12345,
  "pouchdb_update_seq": "67890"
}
```

### 3c: `backup` — `GET /api/system/backup?type=minimal|full`

Query params:
```rust
#[derive(Deserialize)]
pub struct BackupQuery {
    #[serde(rename = "type", default = "default_backup_type")]
    pub backup_type: String,
}
fn default_backup_type() -> String { "minimal".to_string() }
```

Implementation:

1. Validate `backup_type` is `"minimal"` or `"full"`, return 400 otherwise
2. Fetch all documents via `state.db.all_docs(true)`
3. Filter out design documents (`_id` starts with `_design/`)
4. For **minimal** backups, filter documents by type:

   | Included Type | Notes |
   |---|---|
   | `virtual_directory` | Full document |
   | `label_assignment` | Full document |
   | `label_rule` | Full document |
   | `annotation` | Full document (expensive to regenerate via AI/OCR) |
   | `credential` | Full document (secret_key_hash is Argon2id — safe) |
   | `plugin` | Redact secret settings (see below) |
   | `storage_backend` | Full document |
   | `replication_rule` | Full document |
   | `node` | **Partial only:** keep `_id`, `type`, `friendly_name`, `network_mounts` |

5. For **full** backups, include all documents
6. Strip `_rev` from every document (revisions are instance-specific; `_bulk_docs` with `new_edits: false` is not needed since we want fresh revisions on restore)
7. **Plugin secret redaction** (minimal and full): for documents with `type == "plugin"`, walk the `settings` object. For any key where the plugin schema declares `type: "secret"`, replace the value with `"__REDACTED__"`. Since plugin schemas are stored in the plugin document itself (`settings_schema`), iterate schema fields and check for `"type": "secret"`. If no schema is present, leave settings as-is (conservative approach)
8. Build response body: `{ "docs": [...] }`
9. Set response headers:
   - `Content-Type: application/json`
   - `Content-Disposition: attachment; filename="mosaicfs-backup-{type}-{timestamp}.json"` where timestamp is ISO 8601 (e.g. `2026-02-24T15-30-00Z` — use hyphens instead of colons for filename safety)

**Return type:** Use `axum::response::Response` directly to set custom headers, or use `(StatusCode, HeaderMap, Json<...>)`.

### 3d: `backup_status` — `GET /api/system/backup/status`

Returns whether the database is empty (safe for restore).

```rust
pub async fn backup_status(State(state): State<Arc<AppState>>) -> impl IntoResponse
```

Implementation:
- Call `state.db.db_info()`
- Extract `doc_count` from the response
- Subtract design document count (query `_all_docs?startkey="_design/"&endkey="_design/\uffff"&limit=0` or just count them) — alternatively, just count non-design docs by checking if any `all_docs_by_prefix` returns rows for common prefixes like `file::`, `node::`, etc.
- Simpler approach: call `all_docs(false)` and count rows whose `id` doesn't start with `_design/`

Response:
```json
{ "empty": true, "document_count": 0 }
```

### 3e: `restore` — `POST /api/system/restore`

Accept a JSON body containing the backup.

```rust
pub async fn restore(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse
```

Implementation:

1. **Validate format:** body must have a `"docs"` key that is an array. Return 400 if not.
2. **Check empty database:** Call `backup_status` logic. If `document_count > 0`, return 409 Conflict:
   ```json
   { "error": { "code": "database_not_empty", "message": "Restore only permitted into an empty database. Use developer mode wipe or recreate the stack." } }
   ```
3. **Validate document types:** Iterate all docs. Each must have a `"type"` field with a recognized value. Recognized types:
   ```
   file, node, virtual_directory, credential, label_assignment, label_rule,
   plugin, annotation, storage_backend, replication_rule, replica, access,
   agent_status, utilization_snapshot, notification
   ```
   If any document has an unrecognized type, return 400 with the offending type listed.

4. **Separate node partials from full documents:**
   - Documents with `type == "node"` that have only `_id`, `type`, `friendly_name`, and `network_mounts` (no `status`, `last_heartbeat`, etc.) are "partial node documents" from a minimal backup.
   - For partial nodes: do **not** bulk-write them directly. Instead, store the `network_mounts` and `friendly_name` keyed by node `_id` for later merging. These will be applied when the agent re-registers (or can be written as skeleton node docs that the agent's heartbeat will merge into).
   - For full nodes: write directly.
   - **Practical approach:** Write all node documents as-is. When the agent re-registers, it already does a get-or-create check in `node::register_node()` which merges into existing documents. Partial node docs will gain `status`, `last_heartbeat`, etc. from the first heartbeat. This avoids complex merge logic.

5. **Prepare documents for bulk write:**
   - Strip `_rev` from all documents (ensure clean insert)
   - Strip `_conflicts` if present
   - Keep `_id` intact (deterministic IDs are the backbone of the system)

6. **Bulk write:** Call `state.db.bulk_docs(&docs)` in batches of 500 (CouchDB performs better with bounded batch sizes)

7. **Rebuild caches:** After restore, the materialized label cache and access cache are stale. Call:
   ```rust
   state.label_cache.build(&state.db).await?;
   state.access_cache.build(&state.db).await?;
   ```

8. **Recreate indexes:** Call `couchdb::create_indexes(&state.db)` to ensure Mango indexes exist.

9. **Return summary:**
   ```json
   { "ok": true, "restored_count": 1234, "errors": [] }
   ```
   If some documents fail in `bulk_docs`, collect their IDs and error reasons in the `errors` array.

### 3f: `delete_data` — `DELETE /api/system/data`

Developer-mode-only endpoint.

```rust
pub async fn delete_data(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse
```

Implementation:

1. Check `state.developer_mode`. If false, return 403:
   ```json
   { "error": { "code": "forbidden", "message": "Developer mode is not enabled" } }
   ```
2. Validate confirmation token: `body["confirm"]` must equal `"DELETE_ALL_DATA"`. Return 400 otherwise.
3. Delete and recreate the database:
   ```rust
   state.db.delete_db().await?;
   state.db.ensure_db().await?;
   couchdb::create_indexes(&state.db).await?;
   ```
4. Clear in-memory caches:
   ```rust
   state.label_cache.clear();
   state.access_cache.clear();
   ```
   (Note: `clear()` methods may need to be added to `LabelCache` and `AccessCache`)
5. Return `{ "ok": true }`.

---

## Step 4: Wire routes

**Files:** `mosaicfs-server/src/handlers/mod.rs`, `mosaicfs-server/src/routes.rs`

### 4a: Add module

In `handlers/mod.rs`, add:
```rust
pub mod system;
```

### 4b: Replace stubs in `routes.rs`

Add `system` to the handler imports:
```rust
use crate::handlers::{agent, files, labels, nodes, notifications, replication, search, system, vfs};
```

Replace stub routes:
```rust
// System (was stub_501)
.route("/api/health", get(system::health))
.route("/api/system/info", get(system::system_info))
.route("/api/system/backup", get(system::backup))
.route("/api/system/backup/status", get(system::backup_status))
.route("/api/system/restore", post(system::restore))
.route("/api/system/data", delete(system::delete_data))
```

Note: `/api/health` should arguably be **unauthenticated** (for load balancer probes). Move it to `public_routes` if desired, or keep it behind JWT for simplicity in v1.

Also note: `/api/system/backup/status` is a new route (not a stub replacement).

---

## Step 5: Add cache clearing methods

**Files:** `mosaicfs-server/src/label_cache.rs`, `mosaicfs-server/src/access_cache.rs`

The restore handler needs to reset in-memory caches after writing documents.

### 5a: `LabelCache::clear()`

Add a `clear()` method that empties the internal `RwLock<HashMap>` (or equivalent). Looking at the existing `build()` method, it populates the cache from scratch. We can either:
- Add `clear()` that empties the maps, then call `build()` again, or
- Just call `build()` which overwrites everything (check if it does a full replace or incremental update)

If `build()` does a full replace (creates new maps and swaps), then just calling `build()` after restore is sufficient. If it does incremental updates, add `clear()`.

### 5b: `AccessCache::clear()`

Same approach as `LabelCache`.

---

## Step 6: Web UI — Restore controls

**File:** `web/src/pages/SettingsPage.tsx`

### 6a: Add restore section to `AboutTab`

After the existing Backup card, add a Restore card:

```tsx
// State
const [backupStatus, setBackupStatus] = useState<{ empty: boolean; document_count: number } | null>(null);
const [restoreFile, setRestoreFile] = useState<File | null>(null);
const [restoring, setRestoring] = useState(false);
const [restoreResult, setRestoreResult] = useState<{ ok: boolean; restored_count: number; errors: string[] } | null>(null);
```

On mount, fetch `/api/system/backup/status` to determine if restore is available.

**Restore card content (conditionally rendered):**

When `backupStatus?.empty === true`:
- Heading: "Restore"
- Description: "Upload a backup file to restore this instance."
- File input (`<input type="file" accept=".json" />`)
- "Restore" button (disabled until file selected, shows spinner while loading)
- On submit:
  1. Read file as text via `FileReader`
  2. Parse JSON
  3. `POST /api/system/restore` with the parsed JSON body
  4. Show result (success count or errors)

When `backupStatus?.empty === false`:
- Show "Database has {document_count} documents. Restore is only available on an empty database."

When `restoreResult?.ok === true`:
- Show success banner: "Restored {restored_count} documents. Restart all agents to complete recovery."
- Show any errors from the `errors` array

### 6b: Add import

Add `Upload` from `lucide-react` to the imports (for the restore button icon).

### 6c: Update `SystemInfo` interface

The existing `SystemInfo` interface may need updating to match the actual response from `GET /api/system/info`. Check and align.

---

## Step 7: Developer mode wipe UI (optional)

**File:** `web/src/pages/SettingsPage.tsx`

This step is **optional** for v1. If implemented:

- Only show when the server reports developer mode is active (add `developer_mode: boolean` to `/api/system/info` response)
- Red "Delete All Data" button with a confirmation dialog
- Calls `DELETE /api/system/data` with `{ "confirm": "DELETE_ALL_DATA" }`
- After success, refresh backup status (which should now show `empty: true`)

If not implemented in the UI, the endpoint still works via `curl` for development use.

---

## Files to create

| File | Description |
|---|---|
| `mosaicfs-server/src/handlers/system.rs` | Backup, restore, health, info, and data-wipe handlers |

## Files to modify

| File | Change |
|---|---|
| `mosaicfs-server/src/couchdb.rs` | Add `all_docs()`, `db_info()`, `delete_db()` methods |
| `mosaicfs-server/src/state.rs` | Add `developer_mode: bool` field |
| `mosaicfs-server/src/main.rs` | Parse `--developer-mode` flag, pass to AppState, add server start `Instant` for uptime |
| `mosaicfs-server/src/handlers/mod.rs` | Add `pub mod system` |
| `mosaicfs-server/src/routes.rs` | Replace stub_501 routes, add `/api/system/backup/status` and `/api/system/data` |
| `mosaicfs-server/src/label_cache.rs` | Add `clear()` if needed (or verify `build()` is idempotent) |
| `mosaicfs-server/src/access_cache.rs` | Add `clear()` if needed (or verify `build()` is idempotent) |
| `web/src/pages/SettingsPage.tsx` | Add restore file upload, status check, result display |

---

## Document type inclusion matrix

| Type | Minimal | Full | Restore Behavior |
|---|---|---|---|
| `virtual_directory` | Yes | Yes | Bulk write |
| `label_assignment` | Yes | Yes | Bulk write |
| `label_rule` | Yes | Yes | Bulk write |
| `annotation` | Yes | Yes | Bulk write |
| `credential` | Yes | Yes | Bulk write (hash is safe) |
| `plugin` | Yes (secrets redacted) | Yes (secrets redacted) | Bulk write; user re-enters secrets |
| `storage_backend` | Yes | Yes | Bulk write; user re-authorizes OAuth |
| `replication_rule` | Yes | Yes | Bulk write |
| `node` | Partial (`_id`, `type`, `friendly_name`, `network_mounts`) | Yes | Write as-is; agent heartbeat fills in the rest |
| `file` | No | Yes | Bulk write (or agent re-crawl regenerates) |
| `agent_status` | No | Yes | Bulk write |
| `utilization_snapshot` | No | Yes | Bulk write |
| `notification` | No | Yes | Bulk write |
| `replica` | No | Yes | Bulk write |
| `access` | No | Yes | Bulk write |

---

## Plugin secret redaction algorithm

For each document with `type == "plugin"`:

1. Check if `settings_schema` is present and is an object
2. For each key in `settings_schema`, check if the field definition has `"type": "secret"`
3. If so, and `settings[key]` exists, replace `settings[key]` with `"__REDACTED__"`
4. If no `settings_schema` is present, leave `settings` unchanged

Example:
```json
{
  "_id": "plugin::node-abc::ocr",
  "type": "plugin",
  "settings_schema": {
    "api_key": { "type": "secret", "label": "API Key" },
    "model": { "type": "string", "label": "Model" }
  },
  "settings": {
    "api_key": "sk-12345",
    "model": "gpt-4"
  }
}
```

After redaction:
```json
{
  "settings": {
    "api_key": "__REDACTED__",
    "model": "gpt-4"
  }
}
```

---

## Verification checklist

1. **Rust compilation:** `cargo build` succeeds for all workspace members
2. **Existing tests pass:** `cargo test` in workspace root
3. **Web UI builds:** `cd web && npx tsc --noEmit && npx vite build`
4. **Health endpoint:** `GET /api/health` returns `{ "status": "ok" }` when CouchDB is running
5. **System info:** `GET /api/system/info` returns version, uptime, document count
6. **Minimal backup:**
   - `GET /api/system/backup?type=minimal` downloads a JSON file
   - File contains only allowed document types
   - Node documents are partial (only `_id`, `type`, `friendly_name`, `network_mounts`)
   - Plugin secrets are redacted to `"__REDACTED__"`
   - No `_rev` fields present
7. **Full backup:**
   - `GET /api/system/backup?type=full` downloads a JSON file
   - File contains all document types
   - Plugin secrets are still redacted
   - No `_rev` fields present
8. **Backup status:** `GET /api/system/backup/status` returns `{ "empty": false, "document_count": N }`
9. **Restore rejected on non-empty DB:** `POST /api/system/restore` returns 409 when documents exist
10. **Developer mode wipe:**
    - Without `--developer-mode`: `DELETE /api/system/data` returns 403
    - With `--developer-mode`: deletes all data and returns 200
11. **End-to-end restore flow:**
    - Take minimal backup
    - Wipe database (via developer mode or stack recreation)
    - `POST /api/system/restore` with backup JSON
    - Verify virtual directories, credentials, replication rules reappear
    - Agent reconnects, heartbeat succeeds, re-crawl populates files
12. **Web UI restore controls:** Settings > About shows restore section when DB is empty
