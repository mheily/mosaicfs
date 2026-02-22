use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Helper for CouchDB revision tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouchDoc<T> {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub doc: T,
}

// ── File Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileDocument {
    #[serde(rename = "type")]
    pub doc_type: FileType,
    pub inode: u64,
    pub name: String,
    pub source: FileSource,
    pub size: u64,
    pub mtime: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub status: FileStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migrated_from: Option<MigratedFrom>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileType {
    #[serde(rename = "file")]
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "deleted")]
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSource {
    pub node_id: String,
    pub export_path: String,
    pub export_parent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MigratedFrom {
    pub node_id: String,
    pub export_path: String,
    pub migrated_at: DateTime<Utc>,
}

impl FileDocument {
    pub fn new_id() -> String {
        format!("file::{}", Uuid::new_v4())
    }

    pub fn uuid_from_id(id: &str) -> Option<&str> {
        id.strip_prefix("file::")
    }
}

// ── Virtual Directory Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VirtualDirectoryDocument {
    #[serde(rename = "type")]
    pub doc_type: VirtualDirectoryType,
    pub inode: u64,
    pub virtual_path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<bool>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub enforce_steps_on_children: bool,
    #[serde(default)]
    pub mounts: Vec<MountEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VirtualDirectoryType {
    #[serde(rename = "virtual_directory")]
    VirtualDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MountEntry {
    pub mount_id: String,
    pub source: MountSource,
    pub strategy: MountStrategy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_prefix: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default = "default_include")]
    pub default_result: StepResult,
    #[serde(default = "default_conflict_policy")]
    pub conflict_policy: ConflictPolicy,
}

fn default_include() -> StepResult {
    StepResult::Include
}

fn default_conflict_policy() -> ConflictPolicy {
    ConflictPolicy::LastWriteWins
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MountSource {
    Node {
        node_id: String,
        export_path: String,
    },
    Federated {
        federated_import_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MountStrategy {
    #[serde(rename = "prefix_replace")]
    PrefixReplace,
    #[serde(rename = "flatten")]
    Flatten,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepResult {
    #[serde(rename = "include")]
    Include,
    #[serde(rename = "exclude")]
    Exclude,
    #[serde(rename = "continue")]
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConflictPolicy {
    #[serde(rename = "last_write_wins")]
    LastWriteWins,
    #[serde(rename = "suffix_node_id")]
    SuffixNodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub op: String,
    #[serde(default)]
    pub invert: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_match: Option<StepResult>,
    // Op-specific fields stored as extra JSON
    #[serde(flatten)]
    pub params: serde_json::Map<String, serde_json::Value>,
}

// ── Node Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeDocument {
    #[serde(rename = "type")]
    pub doc_type: NodeType,
    pub friendly_name: String,
    pub platform: String,
    pub status: NodeStatus,
    pub last_heartbeat: DateTime<Utc>,
    #[serde(default)]
    pub vfs_capable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vfs_backend: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer: Option<TransferConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<Vec<StorageEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_mounts: Option<Vec<NetworkMount>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    #[serde(rename = "node")]
    Node,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeStatus {
    #[serde(rename = "online")]
    Online,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "degraded")]
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferConfig {
    pub endpoint: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageEntry {
    pub filesystem_id: String,
    pub mount_point: String,
    pub fs_type: String,
    pub device: String,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    #[serde(default)]
    pub watch_paths_on_fs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkMount {
    pub mount_id: String,
    pub remote_node_id: String,
    pub remote_base_export_path: String,
    pub local_mount_path: String,
    pub mount_type: String,
    #[serde(default)]
    pub priority: i32,
}

// ── Credential Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialDocument {
    #[serde(rename = "type")]
    pub doc_type: CredentialType,
    pub access_key_id: String,
    pub secret_key_hash: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
    pub permissions: CredentialPermissions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CredentialType {
    #[serde(rename = "credential")]
    Credential,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialPermissions {
    pub scope: String,
}

// ── Agent Status Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentStatusDocument {
    #[serde(rename = "type")]
    pub doc_type: AgentStatusType,
    pub node_id: String,
    pub updated_at: DateTime<Utc>,
    pub overall: String,
    pub subsystems: serde_json::Value,
    #[serde(default)]
    pub recent_errors: Vec<AgentError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatusType {
    #[serde(rename = "agent_status")]
    AgentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentError {
    pub time: DateTime<Utc>,
    pub subsystem: String,
    pub level: String,
    pub message: String,
}

// ── Utilization Snapshot Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UtilizationSnapshotDocument {
    #[serde(rename = "type")]
    pub doc_type: UtilizationType,
    pub node_id: String,
    pub captured_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystems: Option<Vec<FilesystemSnapshot>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud: Option<CloudSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UtilizationType {
    #[serde(rename = "utilization_snapshot")]
    UtilizationSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilesystemSnapshot {
    pub filesystem_id: String,
    pub mount_point: String,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudSnapshot {
    pub used_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_bytes: Option<u64>,
}

// ── Label Assignment Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabelAssignmentDocument {
    #[serde(rename = "type")]
    pub doc_type: LabelAssignmentType,
    pub file_id: String,
    pub labels: Vec<String>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LabelAssignmentType {
    #[serde(rename = "label_assignment")]
    LabelAssignment,
}

// ── Label Rule Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabelRuleDocument {
    #[serde(rename = "type")]
    pub doc_type: LabelRuleType,
    pub node_id: String,
    pub path_prefix: String,
    pub labels: Vec<String>,
    pub name: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LabelRuleType {
    #[serde(rename = "label_rule")]
    LabelRule,
}

// ── Plugin Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginDocument {
    #[serde(rename = "type")]
    pub doc_type: PluginType,
    pub node_id: String,
    pub plugin_name: String,
    pub plugin_type: String,
    pub enabled: bool,
    pub name: String,
    #[serde(default)]
    pub subscribed_events: Vec<String>,
    #[serde(default)]
    pub mime_globs: Vec<String>,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default = "default_workers")]
    pub workers: i32,
    #[serde(default = "default_timeout")]
    pub timeout_s: i32,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_endpoints: Option<Vec<QueryEndpoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provides_filesystem: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path_prefix: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn default_workers() -> i32 { 2 }
fn default_timeout() -> i32 { 60 }
fn default_max_attempts() -> i32 { 3 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginType {
    #[serde(rename = "plugin")]
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryEndpoint {
    pub name: String,
    pub capability: String,
    pub description: String,
}

// ── Annotation Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationDocument {
    #[serde(rename = "type")]
    pub doc_type: AnnotationType,
    pub file_id: String,
    pub source: AnnotationSource,
    pub plugin_name: String,
    pub data: serde_json::Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub annotated_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnnotationType {
    #[serde(rename = "annotation")]
    Annotation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationSource {
    pub node_id: String,
}

// ── Notification Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationDocument {
    #[serde(rename = "type")]
    pub doc_type: NotificationType,
    pub source: NotificationSource,
    pub severity: String,
    pub status: String,
    pub title: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<NotificationAction>>,
    pub condition_key: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    #[serde(default = "default_occurrence_count")]
    pub occurrence_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

fn default_occurrence_count() -> i64 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NotificationType {
    #[serde(rename = "notification")]
    Notification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationSource {
    pub node_id: String,
    pub component: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationAction {
    pub label: String,
    pub api: String,
}

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

// ── Access Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessDocument {
    #[serde(rename = "type")]
    pub doc_type: AccessType,
    pub file_id: String,
    pub source: AccessSource,
    pub last_access: DateTime<Utc>,
    #[serde(default = "default_occurrence_count")]
    pub access_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessType {
    #[serde(rename = "access")]
    Access,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessSource {
    pub node_id: String,
}

/// Returns the document type string from a JSON value
pub fn document_type(value: &serde_json::Value) -> Option<&str> {
    value.get("type")?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(
        doc: &T,
    ) {
        let json = serde_json::to_string(doc).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(doc, &back);
    }

    fn round_trip_couch<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug + Clone>(
        id: &str,
        doc: T,
    ) {
        let couch = CouchDoc {
            id: id.to_string(),
            rev: Some("1-abc".to_string()),
            doc,
        };
        round_trip(&couch);
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn test_file_document() {
        let doc = FileDocument {
            doc_type: FileType::File,
            inode: 12345,
            name: "report.pdf".to_string(),
            source: FileSource {
                node_id: "node-laptop".to_string(),
                export_path: "/home/user/documents/report.pdf".to_string(),
                export_parent: "/home/user/documents".to_string(),
            },
            size: 204800,
            mtime: now(),
            mime_type: Some("application/pdf".to_string()),
            status: FileStatus::Active,
            deleted_at: None,
            migrated_from: None,
        };
        round_trip_couch("file::abc123", doc);
    }

    #[test]
    fn test_virtual_directory_document() {
        let doc = VirtualDirectoryDocument {
            doc_type: VirtualDirectoryType::VirtualDirectory,
            inode: 1,
            virtual_path: "/".to_string(),
            name: "".to_string(),
            parent_path: None,
            system: Some(true),
            created_at: now(),
            enforce_steps_on_children: false,
            mounts: vec![],
        };
        round_trip_couch("dir::root", doc);
    }

    #[test]
    fn test_node_document() {
        let doc = NodeDocument {
            doc_type: NodeType::Node,
            friendly_name: "MacBook Pro".to_string(),
            platform: "darwin".to_string(),
            status: NodeStatus::Online,
            last_heartbeat: now(),
            vfs_capable: true,
            vfs_backend: None,
            capabilities: vec![],
            transfer: None,
            storage: None,
            network_mounts: None,
        };
        round_trip_couch("node::node-laptop", doc);
    }

    #[test]
    fn test_credential_document() {
        let doc = CredentialDocument {
            doc_type: CredentialType::Credential,
            access_key_id: "MOSAICFS_abc123def456".to_string(),
            secret_key_hash: "argon2id:$argon2id$...".to_string(),
            name: "Main laptop agent".to_string(),
            enabled: true,
            created_at: now(),
            last_seen: None,
            permissions: CredentialPermissions {
                scope: "full".to_string(),
            },
        };
        round_trip_couch("credential::MOSAICFS_abc123def456", doc);
    }

    #[test]
    fn test_agent_status_document() {
        let doc = AgentStatusDocument {
            doc_type: AgentStatusType::AgentStatus,
            node_id: "node-laptop".to_string(),
            updated_at: now(),
            overall: "healthy".to_string(),
            subsystems: serde_json::json!({}),
            recent_errors: vec![],
        };
        round_trip_couch("status::node-laptop", doc);
    }

    #[test]
    fn test_utilization_snapshot_document() {
        let doc = UtilizationSnapshotDocument {
            doc_type: UtilizationType::UtilizationSnapshot,
            node_id: "node-laptop".to_string(),
            captured_at: now(),
            filesystems: Some(vec![FilesystemSnapshot {
                filesystem_id: "fs-1".to_string(),
                mount_point: "/".to_string(),
                used_bytes: 500_000_000_000,
                available_bytes: 500_000_000_000,
            }]),
            cloud: None,
        };
        round_trip_couch("utilization::node-laptop::2025-11-14T09:00:00Z", doc);
    }

    #[test]
    fn test_label_assignment_document() {
        let doc = LabelAssignmentDocument {
            doc_type: LabelAssignmentType::LabelAssignment,
            file_id: "file::abc123".to_string(),
            labels: vec!["work".to_string(), "important".to_string()],
            updated_at: now(),
            updated_by: "MOSAICFS_abc123def456".to_string(),
        };
        round_trip_couch("label_file::abc123", doc);
    }

    #[test]
    fn test_label_rule_document() {
        let doc = LabelRuleDocument {
            doc_type: LabelRuleType::LabelRule,
            node_id: "node-laptop".to_string(),
            path_prefix: "/home/user/documents/".to_string(),
            labels: vec!["documents".to_string()],
            name: "Work documents".to_string(),
            enabled: true,
            created_at: now(),
        };
        round_trip_couch("label_rule::abc123", doc);
    }

    #[test]
    fn test_plugin_document() {
        let doc = PluginDocument {
            doc_type: PluginType::Plugin,
            node_id: "node-laptop".to_string(),
            plugin_name: "ai-summarizer".to_string(),
            plugin_type: "executable".to_string(),
            enabled: true,
            name: "AI Document Summariser".to_string(),
            subscribed_events: vec!["file.added".to_string()],
            mime_globs: vec!["application/pdf".to_string()],
            config: serde_json::json!({}),
            workers: 2,
            timeout_s: 60,
            max_attempts: 3,
            query_endpoints: None,
            settings_schema: None,
            settings: None,
            provides_filesystem: None,
            file_path_prefix: None,
            created_at: now(),
        };
        round_trip_couch("plugin::node-laptop::ai-summarizer", doc);
    }

    #[test]
    fn test_annotation_document() {
        let doc = AnnotationDocument {
            doc_type: AnnotationType::Annotation,
            file_id: "file::abc123".to_string(),
            source: AnnotationSource {
                node_id: "node-laptop".to_string(),
            },
            plugin_name: "ai-summarizer".to_string(),
            data: serde_json::json!({"summary": "A report"}),
            status: "ok".to_string(),
            error: None,
            annotated_at: now(),
            updated_at: now(),
        };
        round_trip_couch("annotation::abc123::ai-summarizer", doc);
    }

    #[test]
    fn test_notification_document() {
        let doc = NotificationDocument {
            doc_type: NotificationType::Notification,
            source: NotificationSource {
                node_id: "node-laptop".to_string(),
                component: "crawler".to_string(),
            },
            severity: "info".to_string(),
            status: "active".to_string(),
            title: "First crawl complete".to_string(),
            message: "Initial crawl finished.".to_string(),
            actions: None,
            condition_key: "first_crawl_complete".to_string(),
            first_seen: now(),
            last_seen: now(),
            occurrence_count: 1,
            acknowledged_at: None,
            resolved_at: None,
        };
        round_trip_couch("notification::node-laptop::first_crawl_complete", doc);
    }

    #[test]
    fn test_storage_backend_document() {
        let doc = StorageBackendDocument {
            doc_type: StorageBackendType::StorageBackend,
            name: "offsite-backup".to_string(),
            hosting_node_id: None,
            backend: "s3".to_string(),
            mode: "target".to_string(),
            backend_config: serde_json::json!({"bucket": "my-bucket", "prefix": "mosaicfs/", "region": "us-east-1"}),
            credentials_ref: Some("cred-1".to_string()),
            schedule: Some("02:00-06:00".to_string()),
            poll_interval_s: None,
            bandwidth_limit_mbps: Some(50),
            retention: RetentionConfig { keep_deleted_days: 30 },
            remove_unmatched: false,
            cloud_storage: None,
            enabled: true,
            created_at: now(),
        };
        round_trip_couch("storage_backend::offsite-backup", doc);
    }

    #[test]
    fn test_replication_rule_document() {
        let doc = ReplicationRuleDocument {
            doc_type: ReplicationRuleType::ReplicationRule,
            name: "Backup PDFs".to_string(),
            target_name: "offsite-backup".to_string(),
            source: ReplicationRuleSource {
                node_id: "*".to_string(),
                path_prefix: None,
            },
            steps: vec![],
            default_result: StepResult::Include,
            enabled: true,
            created_at: now(),
            updated_at: now(),
        };
        round_trip_couch("repl_rule::abc123", doc);
    }

    #[test]
    fn test_replica_document() {
        let doc = ReplicaDocument {
            doc_type: ReplicaType::Replica,
            file_id: "file::abc123".to_string(),
            target_name: "offsite-backup".to_string(),
            source: ReplicaSource {
                node_id: "node-laptop".to_string(),
            },
            backend: "s3".to_string(),
            remote_key: "mosaicfs/abc12345/report.pdf".to_string(),
            replicated_at: now(),
            source_mtime: now(),
            source_size: 204800,
            checksum: None,
            status: "current".to_string(),
        };
        round_trip_couch("replica::abc123::offsite-backup", doc);
    }

    #[test]
    fn test_access_document() {
        let doc = AccessDocument {
            doc_type: AccessType::Access,
            file_id: "file::abc123".to_string(),
            source: AccessSource {
                node_id: "node-laptop".to_string(),
            },
            last_access: now(),
            access_count: 5,
        };
        round_trip_couch("access::abc123", doc);
    }

    #[test]
    fn test_document_type_dispatch() {
        let file = FileDocument {
            doc_type: FileType::File,
            inode: 1000,
            name: "test.txt".to_string(),
            source: FileSource {
                node_id: "n1".to_string(),
                export_path: "/test.txt".to_string(),
                export_parent: "/".to_string(),
            },
            size: 100,
            mtime: now(),
            mime_type: None,
            status: FileStatus::Active,
            deleted_at: None,
            migrated_from: None,
        };
        let json = serde_json::to_string(&file).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(document_type(&value), Some("file"));
    }
}
