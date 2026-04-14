//! Server-side readdir wrapper.
//!
//! Delegates mount evaluation to `mosaicfs_vfs::readdir::evaluate_readdir`
//! and shapes the result for the REST API. The label/access caches local
//! to the server provide the `StepContext` implementation.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use mosaicfs_common::couchdb::CouchClient;
use mosaicfs_common::documents::{MountEntry, Step};
use mosaicfs_common::steps::StepContext;

pub use mosaicfs_vfs::readdir::{collect_inherited_steps, dir_id_for, ReaddirEntry};

use crate::access_cache::AccessCache;
use crate::label_cache::LabelCache;

/// `StepContext` implementation that pulls labels and access times from the
/// server's in-memory caches. Replica and annotation lookups are deferred.
struct EvalContext<'a> {
    label_cache: &'a LabelCache,
    access_cache: &'a AccessCache,
    replicas: HashMap<String, Vec<(String, String)>>,
    annotations: HashMap<String, Vec<String>>,
}

impl<'a> StepContext for EvalContext<'a> {
    fn has_label(&self, file_uuid: &str, label: &str) -> bool {
        self.label_cache.has_label(file_uuid, label)
    }

    fn last_access(&self, file_id: &str) -> Option<DateTime<Utc>> {
        self.access_cache.last_access(file_id)
    }

    fn has_replica(&self, file_uuid: &str, target: Option<&str>, status: Option<&str>) -> bool {
        if let Some(replicas) = self.replicas.get(file_uuid) {
            for (t, s) in replicas {
                if target.map_or(true, |want| want == t)
                    && status.map_or(true, |want| want == s)
                {
                    return true;
                }
            }
        }
        false
    }

    fn has_annotation(&self, file_uuid: &str, plugin_name: &str) -> bool {
        self.annotations
            .get(file_uuid)
            .map(|v| v.iter().any(|p| p == plugin_name))
            .unwrap_or(false)
    }
}

pub async fn evaluate_readdir(
    db: &CouchClient,
    label_cache: &LabelCache,
    access_cache: &AccessCache,
    mounts: &[MountEntry],
    inherited_steps: &[Step],
    child_dirs: &[String],
) -> Result<Vec<ReaddirEntry>, anyhow::Error> {
    let ctx = EvalContext {
        label_cache,
        access_cache,
        replicas: HashMap::new(),
        annotations: HashMap::new(),
    };

    mosaicfs_vfs::readdir::evaluate_readdir(
        db,
        mounts,
        inherited_steps,
        child_dirs,
        &ctx,
        |label| label_cache.files_with_label(label),
    )
    .await
}

/// Convert `ReaddirEntry` list to the JSON shape the REST API exposes.
pub fn entries_to_json(entries: &[ReaddirEntry]) -> Vec<serde_json::Value> {
    entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "file_id": e.file_id,
                "size": e.size,
                "mtime": e.mtime.to_rfc3339(),
                "mime_type": e.mime_type,
                "source": {
                    "node_id": e.source_node_id,
                    "export_path": e.source_export_path,
                },
                "mount_id": e.mount_id,
                "type": "file",
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entries_to_json() {
        let entries = vec![ReaddirEntry {
            name: "test.txt".to_string(),
            file_id: "file::abc".to_string(),
            inode: 42,
            size: 100,
            mtime: Utc::now(),
            mime_type: Some("text/plain".to_string()),
            source_node_id: "node-1".to_string(),
            source_export_path: "/docs/test.txt".to_string(),
            mount_id: "m1".to_string(),
        }];
        let json = entries_to_json(&entries);
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["name"], "test.txt");
        assert_eq!(json[0]["type"], "file");
    }
}
