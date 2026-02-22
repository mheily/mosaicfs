//! Tiered file access for the VFS layer.
//!
//! Tier 1: Local file on this node (direct read from export_path)
//! Tier 2: Network mount (CIFS/NFS) — translate path via node's network_mounts
//! Tier 3: Cloud sync (iCloud/Google Drive local) — local sync directory
//! Tier 4: Remote agent HTTP fetch — HMAC-signed request to owning agent
//!
//! Each tier is tried in order. If a tier fails, the next one is attempted.

use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::cache::FileCache;
use crate::couchdb::CouchClient;
use crate::inode::FileInode;

/// Result of a tiered access attempt.
#[derive(Debug)]
pub enum AccessResult {
    /// File is available at this local path (Tier 1, 2, 3, or from cache).
    LocalPath(PathBuf),
    /// File needs to be fetched from a remote agent.
    NeedsFetch(FetchInfo),
    /// File is not accessible.
    NotAccessible(String),
}

#[derive(Debug, Clone)]
pub struct FetchInfo {
    pub file_id: String,
    pub node_id: String,
    pub transfer_endpoint: String,
    pub size: u64,
    pub mtime: String,
}

/// Network mount info from a node document.
#[derive(Debug, Clone)]
pub struct NetworkMountInfo {
    pub remote_node_id: String,
    pub remote_base_export_path: String,
    pub local_mount_path: String,
    pub mount_type: String,
}

/// Attempt to resolve a file through the tiered access chain.
pub async fn resolve_file(
    file: &FileInode,
    local_node_id: &str,
    watch_paths: &[PathBuf],
    network_mounts: &[NetworkMountInfo],
    cache: &FileCache,
    db: &CouchClient,
) -> AccessResult {
    let file_uuid = file
        .file_id
        .strip_prefix("file::")
        .unwrap_or(&file.file_id);

    // Tier 1: Local file
    if file.source_node_id == local_node_id {
        let path = Path::new(&file.source_export_path);
        if path.exists() {
            // Containment check: verify the path is under a configured watch path
            if let Ok(canonical) = path.canonicalize() {
                if is_under_watch_path(&canonical, watch_paths) {
                    debug!(file_id = %file.file_id, "Tier 1: local file access");
                    return AccessResult::LocalPath(canonical);
                } else {
                    warn!(
                        file_id = %file.file_id,
                        path = %canonical.display(),
                        "Tier 1: path escapes watch paths, denied"
                    );
                }
            }
        }
    }

    // Check cache first (before remote tiers)
    if let Ok(Some(entry)) = cache.get_entry(file_uuid) {
        let mtime_str = file.mtime.to_rfc3339();
        if entry.mtime == mtime_str && entry.size_on_record == file.size {
            let cached_path = cache.entry_path(file_uuid);
            if cached_path.exists() {
                if entry.block_map.is_none() {
                    // Full-file cache hit
                    debug!(file_id = %file.file_id, "Cache hit (full file)");
                    let _ = cache.touch(file_uuid);
                    return AccessResult::LocalPath(cached_path);
                }
                // Block-mode: check if the requested range is present
                // For now, if the file exists and has any content, return it
                // (the FUSE read handler will handle partial reads)
                debug!(file_id = %file.file_id, "Cache hit (block mode)");
                let _ = cache.touch(file_uuid);
                return AccessResult::LocalPath(cached_path);
            }
        }
    }

    // Tier 2: Network mount (CIFS/NFS)
    for mount in network_mounts {
        if mount.remote_node_id == file.source_node_id
            && (mount.mount_type == "cifs" || mount.mount_type == "nfs")
        {
            if let Some(translated) = translate_network_path(
                &file.source_export_path,
                &mount.remote_base_export_path,
                &mount.local_mount_path,
            ) {
                let path = Path::new(&translated);
                if path.exists() {
                    debug!(
                        file_id = %file.file_id,
                        mount_type = %mount.mount_type,
                        "Tier 2: network mount access"
                    );
                    return AccessResult::LocalPath(path.to_path_buf());
                }
            }
        }
    }

    // Tier 3: Cloud sync local directory (iCloud/Google Drive)
    for mount in network_mounts {
        if mount.remote_node_id == file.source_node_id
            && (mount.mount_type == "icloud_local" || mount.mount_type == "gdrive_local")
        {
            if let Some(translated) = translate_network_path(
                &file.source_export_path,
                &mount.remote_base_export_path,
                &mount.local_mount_path,
            ) {
                let path = Path::new(&translated);
                if path.exists() {
                    // For iCloud, check eviction via extended attribute
                    if mount.mount_type == "icloud_local" && is_icloud_evicted(path) {
                        debug!(
                            file_id = %file.file_id,
                            "Tier 3: iCloud file evicted, falling through to Tier 4"
                        );
                        continue;
                    }
                    debug!(
                        file_id = %file.file_id,
                        mount_type = %mount.mount_type,
                        "Tier 3: cloud sync local access"
                    );
                    return AccessResult::LocalPath(path.to_path_buf());
                }
            }
        }
    }

    // Tier 4: Remote agent fetch
    match get_transfer_endpoint(db, &file.source_node_id).await {
        Some(endpoint) => {
            info!(
                file_id = %file.file_id,
                node_id = %file.source_node_id,
                "Tier 4: remote agent fetch needed"
            );
            AccessResult::NeedsFetch(FetchInfo {
                file_id: file.file_id.clone(),
                node_id: file.source_node_id.clone(),
                transfer_endpoint: endpoint,
                size: file.size,
                mtime: file.mtime.to_rfc3339(),
            })
        }
        None => AccessResult::NotAccessible(format!(
            "File {} on node {} is not accessible (no transfer endpoint)",
            file.file_id, file.source_node_id
        )),
    }
}

/// Check if a file path is under one of the configured watch paths.
fn is_under_watch_path(canonical: &Path, watch_paths: &[PathBuf]) -> bool {
    for wp in watch_paths {
        if let Ok(wp_canonical) = wp.canonicalize() {
            if canonical.starts_with(&wp_canonical) {
                return true;
            }
        }
        // Also try without canonicalize in case the watch path doesn't exist yet
        if canonical.starts_with(wp) {
            return true;
        }
    }
    false
}

/// Translate a remote export_path to a local path via a network mount.
fn translate_network_path(
    export_path: &str,
    remote_base: &str,
    local_mount: &str,
) -> Option<String> {
    let relative = export_path.strip_prefix(remote_base)?;
    Some(format!(
        "{}{}",
        local_mount.trim_end_matches('/'),
        if relative.starts_with('/') {
            relative.to_string()
        } else {
            format!("/{}", relative)
        }
    ))
}

/// Check if an iCloud file has been evicted (macOS extended attribute).
fn is_icloud_evicted(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let attr = CString::new("com.apple.ubiquity.is-evicted").unwrap();
        let mut buf = [0u8; 1];
        let result = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                attr.as_ptr(),
                buf.as_mut_ptr() as *mut libc::c_void,
                1,
                0,
                0,
            )
        };
        result > 0 && buf[0] == 1
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// Look up a node's transfer endpoint from CouchDB.
async fn get_transfer_endpoint(db: &CouchClient, node_id: &str) -> Option<String> {
    let doc_id = format!("node::{}", node_id);
    let doc = db.get_document(&doc_id).await.ok()?;

    // Check node is online
    let status = doc.get("status").and_then(|v| v.as_str())?;
    if status != "online" {
        return None;
    }

    doc.get("transfer")
        .and_then(|t| t.get("endpoint"))
        .and_then(|e| e.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_under_watch_path() {
        let watch_paths = vec![
            PathBuf::from("/home/user/documents"),
            PathBuf::from("/home/user/photos"),
        ];
        assert!(is_under_watch_path(
            Path::new("/home/user/documents/report.pdf"),
            &watch_paths
        ));
        assert!(is_under_watch_path(
            Path::new("/home/user/photos/img.jpg"),
            &watch_paths
        ));
        assert!(!is_under_watch_path(
            Path::new("/etc/passwd"),
            &watch_paths
        ));
        assert!(!is_under_watch_path(
            Path::new("/home/user/other/file.txt"),
            &watch_paths
        ));
    }

    #[test]
    fn test_translate_network_path() {
        assert_eq!(
            translate_network_path(
                "/home/user/documents/report.pdf",
                "/home/user/documents",
                "/mnt/remote/docs"
            ),
            Some("/mnt/remote/docs/report.pdf".to_string())
        );
        assert_eq!(
            translate_network_path(
                "/home/user/documents/sub/file.txt",
                "/home/user/documents",
                "/mnt/remote/docs/"
            ),
            Some("/mnt/remote/docs/sub/file.txt".to_string())
        );
        // Path doesn't match mount base
        assert_eq!(
            translate_network_path(
                "/other/path/file.txt",
                "/home/user/documents",
                "/mnt/remote"
            ),
            None
        );
    }

    #[test]
    fn test_translate_network_path_root() {
        assert_eq!(
            translate_network_path("/data/file.txt", "/data", "/net/share"),
            Some("/net/share/file.txt".to_string())
        );
    }
}
