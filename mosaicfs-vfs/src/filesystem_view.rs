use mosaicfs_common::documents::{FilesystemDocument, NodeAvailability};
use tracing::warn;

use crate::couchdb::CouchClient;

#[derive(Debug, Clone)]
pub struct FilesystemView {
    /// See docs/architecture/07-vfs-access.md for the lazy-resolution invariant.
    pub filesystem_id: String,
    pub owning_node_id: String,
    pub export_root: String,
    pub local_mount_path: Option<String>,
    pub mount_type: String,
}

pub fn load_filesystems(
    db: &CouchClient,
    local_node_id: &str,
) -> anyhow::Result<Vec<FilesystemView>> {
    let rt = tokio::runtime::Runtime::new()?;
    let docs = rt.block_on(async { db.all_docs_by_prefix("filesystem::", true).await })?;

    let mut views = Vec::new();
    for row in docs.rows {
        let doc = match row.doc {
            Some(d) => d,
            None => continue,
        };
        let fs_doc: FilesystemDocument = match serde_json::from_value(doc) {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "Failed to parse filesystem document");
                continue;
            }
        };
        let view = project_to_view(&fs_doc, local_node_id);
        views.push(view);
    }
    Ok(views)
}

pub async fn load_filesystems_async(
    db: &CouchClient,
    local_node_id: &str,
) -> anyhow::Result<Vec<FilesystemView>> {
    let docs = db.all_docs_by_prefix("filesystem::", true).await?;

    let mut views = Vec::new();
    for row in docs.rows {
        let doc = match row.doc {
            Some(d) => d,
            None => continue,
        };
        let fs_doc: FilesystemDocument = match serde_json::from_value(doc) {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "Failed to parse filesystem document");
                continue;
            }
        };
        let view = project_to_view(&fs_doc, local_node_id);
        views.push(view);
    }
    Ok(views)
}

fn project_to_view(doc: &FilesystemDocument, local_node_id: &str) -> FilesystemView {
    let local_availability = doc.availability.iter().find(|a| a.node_id == local_node_id);
    FilesystemView {
        filesystem_id: doc.filesystem_id.clone(),
        owning_node_id: doc.owning_node_id.clone(),
        export_root: doc.export_root.clone(),
        local_mount_path: local_availability.map(|a: &NodeAvailability| a.local_mount_path.clone()),
        mount_type: local_availability.map(|a: &NodeAvailability| a.mount_type.clone())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}