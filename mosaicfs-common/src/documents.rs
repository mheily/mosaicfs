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
    Label {
        label: String,
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
    // Op-specific fields stored as extra JSON.
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
    /// See docs/architecture/07-vfs-access.md for the lazy-resolution invariant.
    pub mount_id: String,
    pub filesystem_id: String,
    pub remote_node_id: String,
    pub remote_base_export_path: String,
    pub local_mount_path: String,
    pub mount_type: String,
    #[serde(default)]
    pub priority: i32,
}

// ── Filesystem Document ──

/// Represents a physical or cloud filesystem that can be exported and mounted.
///
/// See docs/architecture/07-vfs-access.md for the lazy-resolution invariant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FilesystemType {
    #[serde(rename = "filesystem")]
    Filesystem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

fn default_occurrence_count() -> i64 {
    1
}

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

pub fn derive_filesystem_id(owning_node_id: &str, export_root: &str) -> String {
    let s: String = export_root
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let collapsed: String = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("{}::{}", owning_node_id, collapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(doc: &T) {
        let json = serde_json::to_string(doc).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(doc, &back);
    }

    fn round_trip_couch<
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug + Clone,
    >(
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
            storage: None,
            network_mounts: None,
        };
        round_trip_couch("node::node-laptop", doc);
    }

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

    #[test]
    fn test_derive_filesystem_id() {
        assert_eq!(
            derive_filesystem_id("node-laptop", "/home/user"),
            "node-laptop::home-user"
        );
        assert_eq!(derive_filesystem_id("node-host", "/"), "node-host::");
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
