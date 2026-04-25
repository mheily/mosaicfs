use serde::Serialize;
use thiserror::Error;

use crate::state::AppState;

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("File '{0}' not found.")]
    NotFound(String),

    #[error("Cannot open remote file: this server's node ID is not configured.")]
    NoNodeId,

    #[error("Cannot open remote file: this node is not registered in the database.")]
    NodeNotRegistered,

    #[error("No local mount found for node '{source_node_id}'.")]
    NoHostMount { source_node_id: String },
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct OpenTarget {
    pub node_id: String,
    pub local_mount_path: String,
    pub relative_path: String, // never begins with '/'
}

pub async fn open_file_by_id(state: &AppState, file_id: &str) -> Result<OpenTarget, OpenError> {
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

    if state.node_id.as_deref() == Some(&source_node_id) {
        let node_doc = state
            .db
            .get_document(&format!("node::{}", source_node_id))
            .await
            .map_err(|_| OpenError::NodeNotRegistered)?;
        resolve_same_node(&source_node_id, &export_path, &node_doc)
    } else {
        let this_node_id = state.node_id.as_deref().ok_or(OpenError::NoNodeId)?;
        let node_doc = state
            .db
            .get_document(&format!("node::{}", this_node_id))
            .await
            .map_err(|_| OpenError::NodeNotRegistered)?;
        resolve_cross_node(&source_node_id, &export_path, &node_doc)
    }
}

fn resolve_same_node(
    source_node_id: &str,
    export_path: &str,
    node_doc: &serde_json::Value,
) -> Result<OpenTarget, OpenError> {
    let storage = node_doc
        .get("storage")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let best = storage
        .iter()
        .filter_map(|e| {
            let mount_point = e.get("mount_point")?.as_str()?;
            export_path
                .starts_with(mount_point)
                .then(|| (mount_point.len(), mount_point.to_string()))
        })
        .max_by_key(|(len, _)| *len);

    match best {
        Some((_, mount_point)) => {
            let rel = export_path
                .strip_prefix(mount_point.as_str())
                .unwrap_or("");
            let relative_path = rel.trim_start_matches('/').to_string();
            debug_assert!(
                !relative_path.starts_with('/'),
                "relative_path must not begin with /"
            );
            Ok(OpenTarget {
                node_id: source_node_id.to_string(),
                local_mount_path: mount_point,
                relative_path,
            })
        }
        None => Err(OpenError::NoHostMount {
            source_node_id: source_node_id.to_string(),
        }),
    }
}

fn resolve_cross_node(
    source_node_id: &str,
    export_path: &str,
    node_doc: &serde_json::Value,
) -> Result<OpenTarget, OpenError> {
    let mounts = node_doc
        .get("network_mounts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let best = mounts
        .iter()
        .filter(|m| {
            m.get("remote_node_id").and_then(|v| v.as_str()) == Some(source_node_id)
        })
        .filter_map(|m| {
            let remote_base = m.get("remote_base_export_path")?.as_str()?;
            let local_mount = m.get("local_mount_path")?.as_str()?;
            let rel = export_path.strip_prefix(remote_base)?;
            let priority = m.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
            Some((remote_base.len(), priority, rel.to_string(), local_mount.to_string()))
        })
        .max_by_key(|(prefix_len, priority, _, _)| (*prefix_len, *priority));

    match best {
        Some((_, _, rel, local_mount)) => {
            let relative_path = rel.trim_start_matches('/').to_string();
            debug_assert!(
                !relative_path.starts_with('/'),
                "relative_path must not begin with /"
            );
            Ok(OpenTarget {
                node_id: source_node_id.to_string(),
                local_mount_path: local_mount,
                relative_path,
            })
        }
        None => Err(OpenError::NoHostMount {
            source_node_id: source_node_id.to_string(),
        }),
    }
}

/// Normalize a mount path: collapse `//`, trim trailing `/` (except root),
/// then attempt `std::fs::canonicalize` and fall back to the normalized form.
pub fn normalize_mount_path(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    // Collapse consecutive slashes
    let mut collapsed = String::with_capacity(input.len());
    let mut prev_slash = false;
    for ch in input.chars() {
        if ch == '/' {
            if !prev_slash {
                collapsed.push(ch);
            }
            prev_slash = true;
        } else {
            collapsed.push(ch);
            prev_slash = false;
        }
    }
    // Trim trailing slash, preserving bare "/"
    let normalized = if collapsed.len() > 1 {
        collapsed.trim_end_matches('/').to_string()
    } else {
        collapsed
    };
    // Resolve symlinks when the path exists on this host
    match std::fs::canonicalize(&normalized) {
        Ok(canonical) => canonical.to_string_lossy().into_owned(),
        Err(_) => normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── T1.2: Cross-node exact match ─────────────────────────────────────────

    #[test]
    fn t1_2_cross_node_exact_match() {
        let node_doc = json!({
            "network_mounts": [{
                "remote_node_id": "node-B",
                "remote_base_export_path": "/export",
                "local_mount_path": "/Volumes/NAS",
                "priority": 0
            }]
        });
        let result = resolve_cross_node("node-B", "/export/photos/a.jpg", &node_doc);
        let target = result.unwrap();
        assert_eq!(target.local_mount_path, "/Volumes/NAS");
        assert_eq!(target.relative_path, "photos/a.jpg");
        assert_eq!(target.node_id, "node-B");
    }

    // ── T1.3: Cross-node longest-prefix match ────────────────────────────────

    #[test]
    fn t1_3_cross_node_longest_prefix() {
        let node_doc = json!({
            "network_mounts": [
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export",
                    "local_mount_path": "/Volumes/A",
                    "priority": 0
                },
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export/photos",
                    "local_mount_path": "/Volumes/B",
                    "priority": 0
                }
            ]
        });
        let result = resolve_cross_node("node-B", "/export/photos/x", &node_doc);
        let target = result.unwrap();
        assert_eq!(target.local_mount_path, "/Volumes/B");
        assert_eq!(target.relative_path, "x");
    }

    // ── T1.4: Cross-node priority tiebreak ──────────────────────────────────

    #[test]
    fn t1_4_cross_node_priority_tiebreak() {
        let node_doc = json!({
            "network_mounts": [
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export",
                    "local_mount_path": "/Volumes/Low",
                    "priority": 1
                },
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export",
                    "local_mount_path": "/Volumes/High",
                    "priority": 10
                }
            ]
        });
        let result = resolve_cross_node("node-B", "/export/file.txt", &node_doc);
        let target = result.unwrap();
        assert_eq!(target.local_mount_path, "/Volumes/High");
    }

    // ── T1.5: Cross-node no matching mount ──────────────────────────────────

    #[test]
    fn t1_5_cross_node_no_mount() {
        let node_doc = json!({ "network_mounts": [] });
        let result = resolve_cross_node("node-B", "/export/file.txt", &node_doc);
        assert!(matches!(result, Err(OpenError::NoHostMount { source_node_id }) if source_node_id == "node-B"));
    }

    // ── T1.7: Same-node single storage entry ────────────────────────────────

    #[test]
    fn t1_7_same_node_single_entry() {
        let node_doc = json!({
            "storage": [{ "mount_point": "/data", "filesystem_id": "fs-1" }]
        });
        let result = resolve_same_node("node-A", "/data/x.txt", &node_doc);
        let target = result.unwrap();
        assert_eq!(target.local_mount_path, "/data");
        assert_eq!(target.relative_path, "x.txt");
        assert_eq!(target.node_id, "node-A");
    }

    // ── T1.8: Same-node longest prefix ──────────────────────────────────────

    #[test]
    fn t1_8_same_node_longest_prefix() {
        let node_doc = json!({
            "storage": [
                { "mount_point": "/data", "filesystem_id": "fs-1" },
                { "mount_point": "/data/archive", "filesystem_id": "fs-2" }
            ]
        });
        let result = resolve_same_node("node-A", "/data/archive/old.txt", &node_doc);
        let target = result.unwrap();
        assert_eq!(target.local_mount_path, "/data/archive");
        assert_eq!(target.relative_path, "old.txt");
    }

    // ── T1.9: Same-node no matching storage entry ────────────────────────────

    #[test]
    fn t1_9_same_node_no_entry() {
        let node_doc = json!({
            "storage": [{ "mount_point": "/other", "filesystem_id": "fs-1" }]
        });
        let result = resolve_same_node("node-A", "/data/x.txt", &node_doc);
        assert!(matches!(result, Err(OpenError::NoHostMount { .. })));
    }

    // ── T1.10: normalize_mount_path ──────────────────────────────────────────

    #[test]
    fn t1_10_normalize_mount_path() {
        assert_eq!(normalize_mount_path(""), "");
        assert_eq!(normalize_mount_path("/"), "/");
        // Non-existent paths pass through without canonicalization
        assert_eq!(normalize_mount_path("/foo//bar/"), "/foo/bar");
        assert_eq!(normalize_mount_path("/foo//bar"), "/foo/bar");
    }

    // ── T1.10a: relative_path invariant across all success cases ─────────────

    #[test]
    fn t1_10a_relative_path_no_leading_slash() {
        let cross_node = json!({
            "network_mounts": [
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export",
                    "local_mount_path": "/Volumes/A",
                    "priority": 0
                },
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export/photos",
                    "local_mount_path": "/Volumes/B",
                    "priority": 0
                },
                {
                    "remote_node_id": "node-B",
                    "remote_base_export_path": "/export/photos",
                    "local_mount_path": "/Volumes/B-hi",
                    "priority": 5
                }
            ]
        });
        let same_node = json!({
            "storage": [
                { "mount_point": "/data", "filesystem_id": "fs-1" },
                { "mount_point": "/data/archive", "filesystem_id": "fs-2" }
            ]
        });

        // T1.2
        let t = resolve_cross_node("node-B", "/export/photos/a.jpg", &cross_node).unwrap();
        assert!(!t.relative_path.starts_with('/'), "T1.2: {}", t.relative_path);

        // T1.3
        let t = resolve_cross_node("node-B", "/export/photos/x", &cross_node).unwrap();
        assert!(!t.relative_path.starts_with('/'), "T1.3: {}", t.relative_path);

        // T1.4
        let t = resolve_cross_node("node-B", "/export/photos/file.txt", &cross_node).unwrap();
        assert!(!t.relative_path.starts_with('/'), "T1.4: {}", t.relative_path);

        // T1.7
        let t = resolve_same_node("node-A", "/data/x.txt", &same_node).unwrap();
        assert!(!t.relative_path.starts_with('/'), "T1.7: {}", t.relative_path);

        // T1.8
        let t = resolve_same_node("node-A", "/data/archive/old.txt", &same_node).unwrap();
        assert!(!t.relative_path.starts_with('/'), "T1.8: {}", t.relative_path);
    }
}
