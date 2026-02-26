use std::path::PathBuf;

use chrono::Utc;
use tracing::{info, warn};

use crate::couchdb::{CouchClient, CouchError};

/// Write or update the node document on startup
pub async fn register_node(
    db: &CouchClient,
    node_id: &str,
    watch_paths: &[PathBuf],
    file_server_url: &str,
    agent_token: &str,
) -> anyhow::Result<()> {
    let doc_id = format!("node::{}", node_id);
    let friendly_name = hostname();
    let platform = current_platform();

    let storage = collect_storage_info(watch_paths);

    let existing = match db.get_document(&doc_id).await {
        Ok(doc) => Some(doc),
        Err(CouchError::NotFound(_)) => None,
        Err(e) => return Err(e.into()),
    };

    let mut doc = if let Some(mut existing) = existing {
        existing["friendly_name"] = serde_json::Value::String(friendly_name);
        existing["platform"] = serde_json::Value::String(platform);
        existing["status"] = serde_json::Value::String("online".to_string());
        existing["last_heartbeat"] = serde_json::Value::String(Utc::now().to_rfc3339());
        existing["file_server_url"] = serde_json::Value::String(file_server_url.to_string());
        existing["agent_token"] = serde_json::Value::String(agent_token.to_string());
        if let Some(s) = storage {
            existing["storage"] = s;
        }
        existing
    } else {
        let mut doc = serde_json::json!({
            "_id": doc_id,
            "type": "node",
            "friendly_name": friendly_name,
            "platform": platform,
            "status": "online",
            "last_heartbeat": Utc::now().to_rfc3339(),
            "vfs_capable": cfg!(target_os = "linux") || cfg!(target_os = "macos"),
            "capabilities": [],
            "file_server_url": file_server_url,
            "agent_token": agent_token,
        });
        if let Some(s) = storage {
            doc["storage"] = s;
        }
        doc
    };

    db.put_document(&doc_id, &doc).await?;
    info!(node_id = %node_id, "Node document registered");
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
        }
        Err(e) => {
            warn!(error = %e, "Failed to set node offline");
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
