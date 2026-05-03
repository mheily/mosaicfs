Frozen at commit e44e1de. Source files listed below; recover with git show e44e1de:<path>.

Recovery files:
- `mosaicfs-common/src/documents.rs` (ReplicaDocument, ReplicaType, ReplicaSource, ReplicationRuleDocument, ReplicationRuleType, ReplicationRuleSource, StorageBackendDocument, StorageBackendType, RetentionConfig, test_replica_document, test_replication_rule_document, test_storage_backend_document)
- `mosaicfs-common/src/replication.rs`
- `mosaicfs-common/src/backend.rs` (BackendAdapter trait, remote_key helper)
- `mosaicfs-agent/src/replication.rs`
- `mosaicfs-agent/src/replication_subsystem.rs`
- `mosaicfs-agent/src/backend/mod.rs`
- `mosaicfs-agent/src/backend/agent_target.rs`
- `mosaicfs-agent/src/backend/directory.rs`
- `mosaicfs-agent/src/backend/s3.rs`
- `mosaicfs-server/src/handlers/replication.rs`
- `mosaicfs-server/templates/replication.html`
- `mosaicfs-server/templates/replication_panel.html`
- `tests/integration/test_06_replication.sh`

---

# Future: Replication

## Intent

The replication subsystem was designed to copy files from indexed nodes to
off-node storage backends (S3, Backblaze B2, local directories, or remote
agent nodes). Administrators would define replication rules that match files
by path prefix and step-pipeline filters, then associate each rule with a
named storage backend. The agent's rule engine would evaluate rules against
crawled file events, schedule uploads respecting configurable bandwidth limits
and schedules, and record the result as `ReplicaDocument`s. A restore
workflow allowed recovering files from a backend back to local disk.

The design was removed before change 016 (CouchDB → SQLite migration) because
the integration test environment (`tests/docker-compose.integration.yml`) did
not build, making it impossible to verify the feature worked end-to-end.
Reintroduce after change 016 lands, with fresh SQL schemas designed alongside
the code, fresh integration tests against the SQLite-backed harness, and
honest unit-test coverage for the rule engine and backend implementations.

## Struct definitions

```rust
// ── Storage Backend Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageBackendDocument {
    #[serde(rename = "type")]
    pub doc_type: StorageBackendType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosting_node_id: Option<String>,
    pub backend: String,
    pub mode: String,
    pub backend_config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_interval_s: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bandwidth_limit_mbps: Option<i32>,
    pub retention: RetentionConfig,
    #[serde(default)]
    pub remove_unmatched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_storage: Option<serde_json::Value>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StorageBackendType {
    #[serde(rename = "storage_backend")]
    StorageBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetentionConfig {
    pub keep_deleted_days: i32,
}

// ── Replication Rule Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicationRuleDocument {
    #[serde(rename = "type")]
    pub doc_type: ReplicationRuleType,
    pub name: String,
    pub target_name: String,
    pub source: ReplicationRuleSource,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default = "default_include")]
    pub default_result: StepResult,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReplicationRuleType {
    #[serde(rename = "replication_rule")]
    ReplicationRule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicationRuleSource {
    pub node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
}

// ── Replica Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicaDocument {
    #[serde(rename = "type")]
    pub doc_type: ReplicaType,
    pub file_id: String,
    pub target_name: String,
    pub source: ReplicaSource,
    pub backend: String,
    pub remote_key: String,
    pub replicated_at: DateTime<Utc>,
    pub source_mtime: DateTime<Utc>,
    pub source_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReplicaType {
    #[serde(rename = "replica")]
    Replica,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicaSource {
    pub node_id: String,
}
```

Note: `Step` and `StepResult` (used by `ReplicationRuleDocument.steps`) still
exist in `mosaicfs-common/src/documents.rs` because they are also used by
`VirtualDirectoryDocument.mounts` for VFS step-pipeline filtering.

## Backend adapter trait

The `BackendAdapter` trait (formerly `mosaicfs-common/src/backend.rs`) defined
the interface each storage backend had to implement:

```rust
#[async_trait::async_trait]
pub trait BackendAdapter: Send + Sync {
    async fn upload(&self, remote_key: &str, data: bytes::Bytes) -> anyhow::Result<()>;
    async fn download(&self, remote_key: &str) -> anyhow::Result<bytes::Bytes>;
    async fn delete(&self, remote_key: &str) -> anyhow::Result<()>;
    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>>;
}
```

Three implementations existed in `mosaicfs-agent/src/backend/`:
- `s3.rs` — S3-compatible (AWS S3, Backblaze B2) via `aws-sdk-s3`
- `directory.rs` — local directory on the agent host
- `agent_target.rs` — remote MosaicFS agent node (proxied transfers)

`mosaicfs-agent/src/backend/mod.rs` contained a `from_backend_doc` factory
that instantiated the right adapter from a `StorageBackendDocument`.

## Rule engine and worker pool

`mosaicfs-agent/src/replication_subsystem.rs` (1,243 LOC) was the core:
- Consumed `FileEvent`s from the crawler via a channel
- Loaded and evaluated replication rules against each event using the
  step pipeline (`mosaicfs-common/src/steps.rs`)
- Managed a worker pool that performed uploads, respecting per-backend
  bandwidth limits (token bucket algorithm) and configurable schedules
- Persisted state to a local `replication.db` (SQLite) alongside the main
  data directory
- Exposed a `ReplicationHandle` that the crawler used to dispatch events
- Unit-tested utility functions: `parse_schedule`, `in_schedule_window`,
  `TokenBucket`

## CouchDB-to-CouchDB replication

`mosaicfs-agent/src/replication.rs` (86 LOC) set up continuous push/pull
replication between this node's local CouchDB and a designated control-plane
CouchDB. This was the federation mechanism; it is entirely replaced by the
intent-log sync protocol designed in change 016.

## Route shapes

Storage backend CRUD (11 routes, all real implementations in `replication.rs`):

```
GET    /api/storage-backends
POST   /api/storage-backends
GET    /api/storage-backends/{name}
PATCH  /api/storage-backends/{name}
DELETE /api/storage-backends/{name}

GET    /api/replication/rules
POST   /api/replication/rules
GET    /api/replication/rules/{rule_id}
PATCH  /api/replication/rules/{rule_id}
DELETE /api/replication/rules/{rule_id}

GET    /api/replication/replicas
GET    /api/replication/status
POST   /api/replication/restore
GET    /api/replication/restore/history
GET    /api/replication/restore/{job_id}
POST   /api/replication/restore/{job_id}/cancel
```

UI actions (in `mosaicfs-server/src/ui/actions.rs`):

```
POST   /ui/storage-backends/create
POST   /ui/storage-backends/{name}/delete
POST   /ui/replication/restore/initiate
POST   /ui/replication/restore/{job_id}/cancel
```

## CouchDB document ID schemes

- Storage backend: `storage_backend::<name>`
- Replication rule: `replication_rule::<uuid>`
- Replica: `replica::<file_uuid>::<target_name>`

## SQLite schemas (from change 016 design-notes §4)

```sql
CREATE TABLE replica (
    file_origin_node_id INTEGER NOT NULL,
    file_origin_file_id INTEGER NOT NULL,
    target_name TEXT NOT NULL,
    origin_node_id INTEGER NOT NULL,
    backend TEXT NOT NULL,
    remote_key TEXT NOT NULL,
    replicated_at TEXT NOT NULL,
    source_mtime TEXT NOT NULL,
    source_size INTEGER NOT NULL,
    checksum TEXT,
    status TEXT NOT NULL,
    PRIMARY KEY (file_origin_node_id, file_origin_file_id, target_name)
);
CREATE INDEX idx_replica_origin ON replica(origin_node_id);

CREATE TABLE replication_rule (
    rule_id TEXT PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    target_name TEXT NOT NULL,
    source_node_id INTEGER,
    source_path_prefix TEXT,
    steps TEXT NOT NULL,             -- JSON array
    default_result TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE storage_backend (
    name TEXT PRIMARY KEY,
    origin_node_id INTEGER NOT NULL,
    hosting_node_id INTEGER,
    backend TEXT NOT NULL,
    mode TEXT NOT NULL,
    backend_config TEXT NOT NULL,    -- JSON
    credentials_ref TEXT,
    schedule TEXT,
    poll_interval_s INTEGER,
    bandwidth_limit_mbps INTEGER,
    keep_deleted_days INTEGER NOT NULL,
    remove_unmatched INTEGER NOT NULL,
    cloud_storage TEXT,              -- JSON, nullable
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
```
