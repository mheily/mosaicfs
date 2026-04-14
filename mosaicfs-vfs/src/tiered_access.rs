//! Tiered file access for the VFS layer.
//!
//! Tier 1: Local file on this node (direct read from export_path)
//! Tier 2: Network mount (CIFS/NFS) — translate path via node's network_mounts
//! Tier 3: Cloud sync (iCloud/Google Drive local) — local sync directory
//! Replica failover: fetch from a replica target when no node-local path satisfies
//!                   the open (directory, S3, B2 backends).
//!
//! Each tier is tried in order. If a tier fails, the next one is attempted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::cache::FileCache;
use mosaicfs_common::couchdb::CouchClient;
pub use crate::filesystem_view::FilesystemView;
use crate::inode::FileInode;

/// Result of a tiered access attempt.
#[derive(Debug)]
pub enum AccessResult {
    /// File is available at this local path (Tier 1, 2, 3, or from cache).
    LocalPath(PathBuf),
    /// File is not accessible.
    NotAccessible(String),
}

/// Attempt to resolve a file through the tiered access chain.
pub async fn resolve_file(
    file: &FileInode,
    local_node_id: &str,
    watch_paths: &[PathBuf],
    filesystems: &[FilesystemView],
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

    // Tier 2/3: Find a FilesystemView whose owning_node_id matches the file's
    // source_node_id and whose export_root is a prefix of source_export_path.
    for fs in filesystems {
        if fs.owning_node_id != file.source_node_id {
            continue;
        }
        if !file.source_export_path.starts_with(&fs.export_root) {
            continue;
        }
        let local_mount = match &fs.local_mount_path {
            Some(m) => m,
            None => continue,
        };
        if let Some(translated) = translate_network_path(
            &file.source_export_path,
            &fs.export_root,
            local_mount,
        ) {
            let path = Path::new(&translated);
            if path.exists() {
                // For iCloud, check eviction via extended attribute
                if fs.mount_type == "icloud_local" && is_icloud_evicted(path) {
                    debug!(
                        file_id = %file.file_id,
                        "Tier 3: iCloud file evicted, falling through to replica failover"
                    );
                    continue;
                }
                debug!(
                    file_id = %file.file_id,
                    mount_type = %fs.mount_type,
                    "Tier 2/3: filesystem mount access"
                );
                return AccessResult::LocalPath(path.to_path_buf());
            }
        }
    }

    // No node-local access path; fall through to replica failover.
    resolve_from_replica(file, db, watch_paths, cache).await
}

/// Public entry point for replica failover called from fuse_fs.
pub async fn resolve_from_replica_for_open(
    file: &FileInode,
    local_node_id: &str,
    watch_paths: &[PathBuf],
    filesystems: &[FilesystemView],
    cache: &FileCache,
    db: &CouchClient,
) -> AccessResult {
    let _ = (local_node_id, filesystems);
    resolve_from_replica(file, db, watch_paths, cache).await
}

/// Tier 4b: Attempt to serve a file from a replica target when the owning
/// node is offline or unreachable.
async fn resolve_from_replica(
    file: &FileInode,
    db: &CouchClient,
    watch_paths: &[PathBuf],
    cache: &FileCache,
) -> AccessResult {
    let file_uuid = file.file_id.strip_prefix("file::").unwrap_or(&file.file_id);

    // Query replica documents for this file
    let replica_prefix = format!("replica::{}", file_uuid);
    let replicas = match db.all_docs_by_prefix(&replica_prefix, true).await {
        Ok(r) => r,
        Err(e) => {
            warn!(file_id = %file.file_id, error = %e, "Tier 4b: failed to query replicas");
            return AccessResult::NotAccessible(format!(
                "File {} not accessible: owning node offline and replica query failed",
                file.file_id
            ));
        }
    };

    // Find a usable replica (prefer "current", accept "frozen")
    let mut best_replica: Option<serde_json::Value> = None;
    for row in replicas.rows {
        let doc = match row.doc { Some(d) => d, None => continue };
        if doc.get("type").and_then(|v| v.as_str()) != Some("replica") { continue; }
        let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if status != "current" && status != "frozen" { continue; }
        let is_current = status == "current";
        if best_replica.is_none() || is_current {
            best_replica = Some(doc);
        }
        if is_current { break; } // Take first "current" replica
    }

    let replica = match best_replica {
        Some(r) => r,
        None => {
            return AccessResult::NotAccessible(format!(
                "File {} not accessible: owning node offline and no current replicas",
                file.file_id
            ));
        }
    };

    let backend_type = replica.get("backend").and_then(|v| v.as_str()).unwrap_or("");
    let remote_key = replica.get("remote_key").and_then(|v| v.as_str()).unwrap_or("");
    let target_name = replica.get("target_name").and_then(|v| v.as_str()).unwrap_or("");

    info!(
        file_id = %file.file_id,
        backend = %backend_type,
        target = %target_name,
        "Tier 4b: serving from replica"
    );

    match backend_type {
        "directory" => {
            // Attempt direct access if the directory is locally mounted
            let backend_doc = match get_backend_doc(db, target_name).await {
                Some(d) => d,
                None => {
                    return AccessResult::NotAccessible(format!(
                        "File {} not accessible: backend '{}' not found",
                        file.file_id, target_name
                    ));
                }
            };

            let dir_path = backend_doc
                .get("backend_config").and_then(|c| c.get("path")).and_then(|v| v.as_str())
                .unwrap_or("/");
            let full_path = PathBuf::from(dir_path).join(remote_key.trim_start_matches('/'));

            if full_path.exists() {
                debug!(file_id = %file.file_id, path = %full_path.display(), "Tier 4b: directory replica access");
                AccessResult::LocalPath(full_path)
            } else {
                AccessResult::NotAccessible(format!(
                    "File {} not accessible: directory replica path {} not found",
                    file.file_id, full_path.display()
                ))
            }
        }

        "s3" | "b2" => {
            // Download from S3/B2 and cache
            let backend_doc = match get_backend_doc(db, target_name).await {
                Some(d) => d,
                None => {
                    return AccessResult::NotAccessible(format!(
                        "File {} not accessible: backend '{}' not found",
                        file.file_id, target_name
                    ));
                }
            };

            let credentials_ref = backend_doc.get("credentials_ref").and_then(|v| v.as_str());
            let credential_doc = if let Some(cref) = credentials_ref {
                db.get_document(&format!("credential::{}", cref)).await.ok()
            } else {
                None
            };

            match download_from_s3(&backend_doc, credential_doc.as_ref(), remote_key).await {
                Ok(data) => {
                    // Cache the downloaded data
                    let staging_path = cache.staging_path();
                    if let Err(e) = std::fs::write(&staging_path, &data) {
                        warn!(error = %e, "Tier 4b: failed to write staging file");
                        return AccessResult::NotAccessible(format!(
                            "File {} not accessible: failed to cache replica content",
                            file.file_id
                        ));
                    }

                    let final_path = cache.entry_path(file_uuid);
                    if let Some(parent) = final_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    if let Err(e) = std::fs::rename(&staging_path, &final_path) {
                        warn!(error = %e, "Tier 4b: failed to rename cached file");
                        return AccessResult::NotAccessible(format!(
                            "File {} not accessible: failed to install replica cache",
                            file.file_id
                        ));
                    }

                    let source = format!("replica:{}", target_name);
                    let mtime = file.mtime.to_rfc3339();
                    let _ = cache.store_full_file(file_uuid, &file.file_id, &mtime, data.len() as u64, &source);

                    info!(file_id = %file.file_id, target = %target_name, "Tier 4b: S3/B2 replica cached and served");
                    AccessResult::LocalPath(final_path)
                }
                Err(e) => {
                    warn!(file_id = %file.file_id, error = %e, "Tier 4b: S3/B2 download failed");
                    AccessResult::NotAccessible(format!(
                        "File {} not accessible: S3/B2 replica download failed: {}",
                        file.file_id, e
                    ))
                }
            }
        }

        _ => AccessResult::NotAccessible(format!(
            "File {} not accessible: unknown replica backend '{}'",
            file.file_id, backend_type
        )),
    }
}

/// Download an object from S3/B2 using AWS Signature V4.
async fn download_from_s3(
    backend_doc: &serde_json::Value,
    credential_doc: Option<&serde_json::Value>,
    remote_key: &str,
) -> anyhow::Result<Vec<u8>> {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    let config = backend_doc.get("backend_config").cloned().unwrap_or_default();
    let bucket = config.get("bucket").and_then(|v| v.as_str()).unwrap_or("");
    let region = config.get("region").and_then(|v| v.as_str()).unwrap_or("us-east-1");
    let endpoint = config.get("endpoint").and_then(|v| v.as_str())
        .map(|e| e.to_string())
        .unwrap_or_else(|| format!("https://s3.{}.amazonaws.com/{}", region, bucket));

    let (access_key_id, secret_access_key) = if let Some(cred) = credential_doc {
        let kid = cred.get("aws_access_key_id")
            .or_else(|| cred.get("access_key_id"))
            .and_then(|v| v.as_str()).unwrap_or("").to_string();
        let secret = cred.get("aws_secret_access_key")
            .or_else(|| cred.get("secret_key"))
            .and_then(|v| v.as_str()).unwrap_or("").to_string();
        (kid, secret)
    } else {
        (
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
        )
    };

    let now = chrono::Utc::now();
    let date_time = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();

    // Compute empty body hash
    let empty_hash = hex::encode(Sha256::digest(b""));

    // Build host header
    let host = {
        let without_scheme = endpoint.strip_prefix("https://")
            .or_else(|| endpoint.strip_prefix("http://"))
            .unwrap_or(&endpoint);
        without_scheme.split('/').next().unwrap_or(without_scheme).to_string()
    };

    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    headers.insert("host".to_string(), host);
    headers.insert("x-amz-content-sha256".to_string(), empty_hash.clone());
    headers.insert("x-amz-date".to_string(), date_time.clone());

    let canonical_headers: String = headers.iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();
    let signed_headers: String = headers.keys().cloned().collect::<Vec<_>>().join(";");

    let canonical_request = format!(
        "GET\n/{}\n\n{}\n{}\n{}",
        remote_key, canonical_headers, signed_headers, empty_hash
    );

    let cr_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let credential_scope = format!("{}/{}/s3/aws4_request", date, region);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{}\n{}\n{}", date_time, credential_scope, cr_hash);

    // Derive signing key
    type HmacSha256 = Hmac<Sha256>;
    let k_date = {
        let mut m = HmacSha256::new_from_slice(format!("AWS4{}", secret_access_key).as_bytes()).unwrap();
        m.update(date.as_bytes());
        m.finalize().into_bytes().to_vec()
    };
    let k_region = {
        let mut m = HmacSha256::new_from_slice(&k_date).unwrap();
        m.update(region.as_bytes());
        m.finalize().into_bytes().to_vec()
    };
    let k_service = {
        let mut m = HmacSha256::new_from_slice(&k_region).unwrap();
        m.update(b"s3");
        m.finalize().into_bytes().to_vec()
    };
    let k_signing = {
        let mut m = HmacSha256::new_from_slice(&k_service).unwrap();
        m.update(b"aws4_request");
        m.finalize().into_bytes().to_vec()
    };

    let signature = {
        let mut m = HmacSha256::new_from_slice(&k_signing).unwrap();
        m.update(string_to_sign.as_bytes());
        hex::encode(m.finalize().into_bytes())
    };

    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
        access_key_id, credential_scope, signed_headers, signature
    );

    let url = format!("{}/{}", endpoint.trim_end_matches('/'), remote_key);
    let client = reqwest::Client::new();
    let resp = client.get(&url)
        .header("x-amz-date", &date_time)
        .header("x-amz-content-sha256", &empty_hash)
        .header("Authorization", &auth)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("S3 GET failed: HTTP {}", status);
    }

    Ok(resp.bytes().await?.to_vec())
}

/// Retrieve a storage backend document from CouchDB.
async fn get_backend_doc(db: &CouchClient, target_name: &str) -> Option<serde_json::Value> {
    db.get_document(&format!("storage_backend::{}", target_name)).await.ok()
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
