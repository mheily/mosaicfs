use std::path::PathBuf;

use chrono::Utc;
use mosaicfs_common::documents::FilesystemDocument;
use tracing::{info, warn};

use mosaicfs_common::couchdb::{CouchClient, CouchError};

/// Write or update the node document on startup
pub async fn register_node(
    db: &CouchClient,
    node_id: &str,
    watch_paths: &[PathBuf],
) -> anyhow::Result<()> {
    let doc_id = format!("node::{}", node_id);
    // Use node_id as the display name (strip "node-" prefix if present so
    // "node-mynashost" shows as "mynashost"). Inside a container the OS hostname
    // is the pod name, not the physical host, so we don't use hostname() here.
    let friendly_name = node_id
        .strip_prefix("node-")
        .unwrap_or(node_id)
        .to_string();
    let platform = current_platform();

    let storage = collect_storage_info(watch_paths);

    let existing = match db.get_document(&doc_id).await {
        Ok(doc) => Some(doc),
        Err(CouchError::NotFound(_)) => None,
        Err(e) => return Err(e.into()),
    };

    let mut doc = if let Some(mut existing) = existing {
        existing["name"] = serde_json::Value::String(friendly_name);
        existing["platform"] = serde_json::Value::String(platform);
        existing["status"] = serde_json::Value::String("online".to_string());
        existing["last_heartbeat"] = serde_json::Value::String(Utc::now().to_rfc3339());
        if let Some(ref s) = storage {
            existing["storage"] = s.clone();
        }
        existing
    } else {
        let mut doc = serde_json::json!({
            "_id": doc_id,
            "type": "node",
            "name": friendly_name,
            "platform": platform,
            "status": "online",
            "last_heartbeat": Utc::now().to_rfc3339(),
            "vfs_capable": cfg!(target_os = "linux") || cfg!(target_os = "macos"),
            "capabilities": [],
        });
        if let Some(ref s) = storage {
            doc["storage"] = s.clone();
        }
        doc
    };

    db.put_document(&doc_id, &doc).await?;
    info!(node_id = %node_id, "Node document registered");

    // Publish filesystem availability for this node's storage entries.
    // Parse the storage JSON we just wrote to extract filesystem IDs and mount points.
    let storage_entries = storage.as_ref().and_then(|s| s.as_array());
    if let Some(entries) = storage_entries {
        let fs_entries: Vec<FilesystemPublishInfo> = entries
            .iter()
            .filter_map(|e| {
                Some(FilesystemPublishInfo {
                    filesystem_id: e.get("filesystem_id")?.as_str()?.to_string(),
                    mount_point: e.get("mount_point")?.as_str()?.to_string(),
                })
            })
            .collect();
        if let Err(e) = publish_filesystem_availability(db, node_id, &fs_entries, &[]).await {
            warn!(error = %e, "Failed to publish filesystem availability on registration");
        }
    }

    Ok(())
}

/// Send a heartbeat update
pub async fn heartbeat(db: &CouchClient, node_id: &str) -> anyhow::Result<()> {
    let doc_id = format!("node::{}", node_id);
    match db.get_document(&doc_id).await {
        Ok(mut doc) => {
            doc["last_heartbeat"] = serde_json::Value::String(Utc::now().to_rfc3339());
            doc["status"] = serde_json::Value::String("online".to_string());
            db.put_document(&doc_id, &doc).await?;

            // Re-publish filesystem availability on heartbeat
            let storage_entries = doc.get("storage").and_then(|s| s.as_array());
            if let Some(entries) = storage_entries {
                let fs_entries: Vec<FilesystemPublishInfo> = entries
                    .iter()
                    .filter_map(|e| {
                        Some(FilesystemPublishInfo {
                            filesystem_id: e.get("filesystem_id")?.as_str()?.to_string(),
                            mount_point: e.get("mount_point")?.as_str()?.to_string(),
                        })
                    })
                    .collect();
                if let Err(e) = publish_filesystem_availability(db, node_id, &fs_entries, &[]).await {
                    warn!(error = %e, "Failed to publish filesystem availability on heartbeat");
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to send heartbeat");
        }
    }
    Ok(())
}

/// Set node status to offline (clean shutdown)
pub async fn set_offline(db: &CouchClient, node_id: &str) -> anyhow::Result<()> {
    let doc_id = format!("node::{}", node_id);
    match db.get_document(&doc_id).await {
        Ok(mut doc) => {
            doc["status"] = serde_json::Value::String("offline".to_string());
            doc["last_heartbeat"] = serde_json::Value::String(Utc::now().to_rfc3339());
            db.put_document(&doc_id, &doc).await?;
            info!(node_id = %node_id, "Node set to offline");

            // Clear this node's availability from all filesystem documents
            if let Err(e) = remove_filesystem_availability(db, node_id).await {
                warn!(error = %e, "Failed to clear filesystem availability on shutdown");
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to set node offline");
        }
    }
    Ok(())
}

struct FilesystemPublishInfo {
    filesystem_id: String,
    mount_point: String,
}

/// Publish filesystem availability for each storage entry and network mount
/// this node knows about.
pub async fn publish_filesystem_availability(
    db: &CouchClient,
    node_id: &str,
    storage: &[FilesystemPublishInfo],
    network_mounts: &[mosaicfs_common::documents::NetworkMount],
) -> anyhow::Result<()> {
    let now = Utc::now();
    for entry in storage {
        let filesystem_id = &entry.filesystem_id;
        let export_root = &entry.mount_point;
        if let Err(e) = upsert_filesystem(
            db,
            node_id,
            filesystem_id,
            node_id,
            export_root,
            "local",
            export_root,
            &now,
        ).await {
            warn!(filesystem_id = %filesystem_id, error = %e, "Failed to upsert filesystem doc");
        }
    }
    for nm in network_mounts {
        let filesystem_id = &nm.filesystem_id;
        if let Err(e) = upsert_filesystem(
            db,
            node_id,
            filesystem_id,
            &nm.remote_node_id,
            &nm.remote_base_export_path,
            &nm.mount_type,
            &nm.local_mount_path,
            &now,
        ).await {
            warn!(filesystem_id = %filesystem_id, error = %e, "Failed to upsert filesystem doc for network mount");
        }
    }
    Ok(())
}

/// Read-modify-write upsert of a filesystem document with conflict retry.
async fn upsert_filesystem(
    db: &CouchClient,
    self_node_id: &str,
    filesystem_id: &str,
    owning_node_id: &str,
    export_root: &str,
    mount_type: &str,
    local_mount_path: &str,
    now: &chrono::DateTime<Utc>,
) -> anyhow::Result<()> {
    let doc_id = FilesystemDocument::doc_id(filesystem_id);
    let friendly_name = filesystem_id
        .strip_prefix("fs-")
        .unwrap_or(filesystem_id)
        .to_string();

    for attempt in 0..3 {
        let mut doc = match db.get_document(&doc_id).await {
            Ok(mut existing) => {
                // Consumer-only update: must not overwrite owning_node_id or export_root
                // if they differ from what we expect.
                let existing_owner = existing.get("owning_node_id").and_then(|v| v.as_str()).unwrap_or("");
                let existing_root = existing.get("export_root").and_then(|v| v.as_str()).unwrap_or("");
                if existing_owner != owning_node_id && !existing_owner.is_empty() {
                    warn!(
                        filesystem_id = %filesystem_id,
                        expected_owner = %owning_node_id,
                        actual_owner = %existing_owner,
                        "Skipping filesystem upsert: owning_node_id mismatch"
                    );
                    return Ok(());
                }
                if existing_root != export_root && !existing_root.is_empty() {
                    warn!(
                        filesystem_id = %filesystem_id,
                        expected_root = %export_root,
                        actual_root = %existing_root,
                        "Skipping filesystem upsert: export_root mismatch"
                    );
                    return Ok(());
                }
                // Replace or add this node's availability row
                if existing.get("availability").is_none() {
                    existing["availability"] = serde_json::json!([]);
                }
                let availability = existing.get_mut("availability")
                    .and_then(|v| v.as_array_mut())
                    .unwrap();
                let mut found = false;
                for row in availability.iter_mut() {
                    if row.get("node_id").and_then(|v| v.as_str()) == Some(self_node_id) {
                        *row = serde_json::json!({
                            "node_id": self_node_id,
                            "local_mount_path": local_mount_path,
                            "mount_type": mount_type,
                            "last_seen": now.to_rfc3339(),
                        });
                        found = true;
                        break;
                    }
                }
                if !found {
                    availability.push(serde_json::json!({
                        "node_id": self_node_id,
                        "local_mount_path": local_mount_path,
                        "mount_type": mount_type,
                        "last_seen": now.to_rfc3339(),
                    }));
                }
                existing
            }
            Err(CouchError::NotFound(_)) => {
                serde_json::json!({
                    "_id": doc_id,
                    "type": "filesystem",
                    "filesystem_id": filesystem_id,
                    "friendly_name": friendly_name,
                    "owning_node_id": owning_node_id,
                    "export_root": export_root,
                    "availability": [{
                        "node_id": self_node_id,
                        "local_mount_path": local_mount_path,
                        "mount_type": mount_type,
                        "last_seen": now.to_rfc3339(),
                    }],
                    "created_at": now.to_rfc3339(),
                })
            }
            Err(e) => return Err(e.into()),
        };

        match db.put_document(&doc_id, &doc).await {
            Ok(_) => return Ok(()),
            Err(CouchError::Conflict(_)) => {
                if attempt < 2 {
                    warn!(filesystem_id = %filesystem_id, attempt = attempt + 1, "Conflict on filesystem upsert, retrying");
                    continue;
                }
                warn!(filesystem_id = %filesystem_id, "Conflict on filesystem upsert after 3 attempts, skipping");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Remove this node's availability row from all filesystem documents.
async fn remove_filesystem_availability(db: &CouchClient, node_id: &str) -> anyhow::Result<()> {
    let fs_docs = match db.all_docs_by_prefix("filesystem::", true).await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "Failed to query filesystem docs for offline cleanup");
            return Err(e.into());
        }
    };

    for row in fs_docs.rows {
        let mut doc = match row.doc {
            Some(d) => d,
            None => continue,
        };
        let availability = match doc.get_mut("availability").and_then(|v| v.as_array_mut()) {
            Some(a) => a,
            None => continue,
        };
        let original_len = availability.len();
        availability.retain(|row| {
            row.get("node_id").and_then(|v| v.as_str()) != Some(node_id)
        });
        if availability.len() < original_len {
            let doc_id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = db.put_document(doc_id, &doc).await {
                warn!(doc_id = %doc_id, error = %e, "Failed to update filesystem doc on offline");
            }
        }
    }
    Ok(())
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| {
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|_| "unknown".to_string())
}

fn current_platform() -> String {
    if cfg!(target_os = "macos") {
        "darwin".to_string()
    } else if cfg!(target_os = "linux") {
        "linux".to_string()
    } else if cfg!(target_os = "windows") {
        "windows".to_string()
    } else {
        "unknown".to_string()
    }
}

fn collect_storage_info(watch_paths: &[PathBuf]) -> Option<serde_json::Value> {
    // Basic storage info - just report watch paths without detailed filesystem info
    // (detailed info like blkid/diskutil requires platform-specific implementation)
    let entries: Vec<serde_json::Value> = watch_paths
        .iter()
        .filter(|p| p.exists())
        .map(|p| {
            serde_json::json!({
                "filesystem_id": p.to_string_lossy(),
                "mount_point": "/",
                "fs_type": "unknown",
                "device": "unknown",
                "capacity_bytes": 0,
                "used_bytes": 0,
                "watch_paths_on_fs": [p.to_string_lossy()],
            })
        })
        .collect();

    if entries.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(entries))
    }
}
