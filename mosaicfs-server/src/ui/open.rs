//! Shared "open by file_id" helper (§3.2).
//!
//! Extracted from `open_file_action` so both the old admin UI and the new
//! file browser can reuse the path-resolution + OS-spawn logic.

use crate::state::AppState;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("File '{0}' not found.")]
    NotFound(String),

    #[error("Cannot open remote file: this server's node ID is not configured.")]
    NoNodeId,

    #[error("Cannot open remote file: this node is not registered in the database.")]
    NodeNotRegistered,

    #[error("Cannot open: no local mount found for node '{0}'. Configure a network mount on this node first.")]
    NoMount(String),

    #[error("Cannot open: path not accessible: {0}")]
    PathNotAccessible(String),

    #[error("Failed to open '{0}': {1}")]
    SpawnFailed(String, String),
}

/// Resolve a file_id to a local path and spawn the OS opener.
/// Returns the local path on success.
pub async fn open_file_by_id(
    state: &AppState,
    file_id: &str,
) -> Result<String, OpenError> {
    let file_doc = state
        .db
        .get_document(file_id)
        .await
        .map_err(|_| OpenError::NotFound(file_id.to_string()))?;

    let source_node_id = file_doc
        .get("source")
        .and_then(|s| s.get("node_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let export_path = file_doc
        .get("source")
        .and_then(|s| s.get("export_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let local_path = if state.node_id.as_deref() == Some(&source_node_id) {
        export_path.clone()
    } else {
        let this_node_id = state
            .node_id
            .clone()
            .ok_or(OpenError::NoNodeId)?;

        let node_doc = state
            .db
            .get_document(&format!("node::{}", this_node_id))
            .await
            .map_err(|_| OpenError::NodeNotRegistered)?;

        let mounts = node_doc
            .get("network_mounts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let best = mounts
            .iter()
            .filter(|m| {
                m.get("remote_node_id").and_then(|v| v.as_str())
                    == Some(source_node_id.as_str())
            })
            .filter_map(|m| {
                let remote_base = m.get("remote_base_export_path").and_then(|v| v.as_str())?;
                let local_mount = m.get("local_mount_path").and_then(|v| v.as_str())?;
                let rel = export_path.strip_prefix(remote_base)?;
                let local = format!(
                    "{}{}",
                    local_mount.trim_end_matches('/'),
                    rel
                );
                let priority = m.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
                Some((remote_base.len(), priority, local))
            })
            .max_by_key(|(prefix_len, priority, _)| (*prefix_len, *priority));

        match best {
            Some((_, _, path)) => path,
            None => return Err(OpenError::NoMount(source_node_id)),
        }
    };

    if !std::path::Path::new(&local_path).exists() {
        return Err(OpenError::PathNotAccessible(local_path));
    }

    let open_cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    std::process::Command::new(open_cmd)
        .arg(&local_path)
        .spawn()
        .map_err(|e| OpenError::SpawnFailed(local_path.clone(), e.to_string()))?;

    Ok(local_path)
}
