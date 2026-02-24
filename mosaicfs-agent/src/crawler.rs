use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use mosaicfs_common::documents::*;
use rand::Rng;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::couchdb::CouchClient;
use crate::replication_subsystem::{FileEvent, ReplicationHandle};

const BATCH_SIZE: usize = 200;

/// Result of a single crawl pass
pub struct CrawlResult {
    pub files_indexed: u64,
    pub files_skipped: u64,
    pub files_new: u64,
    pub files_updated: u64,
    pub files_deleted: u64,
}

/// Crawl all watch paths and sync file documents to CouchDB.
/// Emits FileEvents to the replication subsystem if a handle is provided.
pub async fn crawl(
    db: &CouchClient,
    node_id: &str,
    watch_paths: &[PathBuf],
    excluded_paths: &[PathBuf],
    replication: Option<&ReplicationHandle>,
) -> anyhow::Result<CrawlResult> {
    let mut result = CrawlResult {
        files_indexed: 0,
        files_skipped: 0,
        files_new: 0,
        files_updated: 0,
        files_deleted: 0,
    };

    // Load existing file docs for this node into a map keyed by export_path
    let existing = load_existing_files(db, node_id).await?;
    let mut seen_paths: HashMap<String, bool> = HashMap::new();
    let mut batch: Vec<serde_json::Value> = Vec::new();

    for watch_path in watch_paths {
        if !watch_path.exists() {
            warn!(path = %watch_path.display(), "Watch path does not exist, skipping");
            continue;
        }

        for entry in WalkDir::new(watch_path).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();

            if is_excluded(path, excluded_paths) {
                continue;
            }

            let export_path = path.to_string_lossy().to_string();
            seen_paths.insert(export_path.clone(), true);

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to stat file");
                    continue;
                }
            };

            let size = metadata.len();
            let mtime = system_time_to_chrono(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH));

            // Check if unchanged
            if let Some(existing_doc) = existing.get(&export_path) {
                let existing_size = existing_doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                let existing_mtime_str = existing_doc.get("mtime").and_then(|v| v.as_str()).unwrap_or("");
                let existing_status = existing_doc.get("status").and_then(|v| v.as_str()).unwrap_or("active");

                if existing_status == "active" {
                    if let Ok(existing_mtime) = existing_mtime_str.parse::<DateTime<Utc>>() {
                        if existing_size == size && existing_mtime == mtime {
                            result.files_skipped += 1;
                            continue;
                        }
                    }
                }
            }

            // Build file document
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

            // Reject names with null bytes, forward slashes, or control chars
            if name.contains('\0') || name.contains('/') || name.chars().any(|c| c.is_control()) {
                debug!(path = %path.display(), "Skipping file with invalid name");
                continue;
            }

            let export_parent = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let mime_type = mime_guess::from_path(path).first().map(|m| m.to_string());

            let (doc_id, inode, rev) = if let Some(existing_doc) = existing.get(&export_path) {
                // Reuse existing _id and inode (even if was deleted - preserve inode)
                let id = existing_doc.get("_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let inode = existing_doc.get("inode").and_then(|v| v.as_u64()).unwrap_or_else(random_inode);
                let rev = existing_doc.get("_rev").and_then(|v| v.as_str()).map(|s| s.to_string());
                result.files_updated += 1;
                (id, inode, rev)
            } else {
                result.files_new += 1;
                (FileDocument::new_id(), random_inode(), None)
            };

            let mut doc = serde_json::json!({
                "_id": doc_id,
                "type": "file",
                "inode": inode,
                "name": name,
                "source": {
                    "node_id": node_id,
                    "export_path": export_path,
                    "export_parent": export_parent,
                },
                "size": size,
                "mtime": mtime,
                "status": "active",
            });

            if let Some(mime) = mime_type {
                doc["mime_type"] = serde_json::Value::String(mime);
            }
            let is_new = rev.is_none() || existing.get(&export_path).map_or(false, |d| {
                d.get("status").and_then(|v| v.as_str()) == Some("deleted")
            });

            if let Some(rev) = rev {
                doc["_rev"] = serde_json::Value::String(rev);
            }
            let event_doc = doc.clone();
            let event_file_id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let is_really_new = result.files_new > result.files_updated; // approximate

            batch.push(doc);
            result.files_indexed += 1;

            // Queue replication event (after batch flush for correct doc ordering)
            if let Some(rh) = replication {
                if is_new {
                    rh.send(FileEvent::Added { file_id: event_file_id, file_doc: event_doc });
                } else {
                    rh.send(FileEvent::Modified { file_id: event_file_id, file_doc: event_doc });
                }
            }

            if batch.len() >= BATCH_SIZE {
                flush_batch(db, &mut batch).await?;
            }
        }
    }

    // Flush remaining
    if !batch.is_empty() {
        flush_batch(db, &mut batch).await?;
    }

    // Soft-delete files that no longer exist
    result.files_deleted = soft_delete_missing(db, &existing, &seen_paths, replication).await?;

    info!(
        new = result.files_new,
        updated = result.files_updated,
        skipped = result.files_skipped,
        deleted = result.files_deleted,
        "Crawl complete"
    );

    Ok(result)
}

fn is_excluded(path: &Path, excluded_paths: &[PathBuf]) -> bool {
    excluded_paths.iter().any(|excl| path.starts_with(excl))
}

fn random_inode() -> u64 {
    let mut rng = rand::thread_rng();
    loop {
        let inode: u64 = rng.gen();
        if inode >= 1000 {
            return inode;
        }
    }
}

fn system_time_to_chrono(t: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(t)
}

async fn load_existing_files(
    db: &CouchClient,
    node_id: &str,
) -> anyhow::Result<HashMap<String, serde_json::Value>> {
    let resp = db.all_docs_by_prefix("file::", true).await?;
    let mut map = HashMap::new();
    for row in resp.rows {
        if let Some(doc) = row.doc {
            let doc_node_id = doc
                .get("source")
                .and_then(|s| s.get("node_id"))
                .and_then(|v| v.as_str());
            if doc_node_id == Some(node_id) {
                if let Some(path) = doc
                    .get("source")
                    .and_then(|s| s.get("export_path"))
                    .and_then(|v| v.as_str())
                {
                    map.insert(path.to_string(), doc);
                }
            }
        }
    }
    Ok(map)
}

async fn flush_batch(
    db: &CouchClient,
    batch: &mut Vec<serde_json::Value>,
) -> anyhow::Result<()> {
    let results = db.bulk_docs(batch).await?;
    let errors: Vec<_> = results
        .iter()
        .filter(|r| r.error.is_some())
        .collect();
    if !errors.is_empty() {
        warn!(count = errors.len(), "Some documents failed to write");
        for e in &errors {
            warn!(
                id = ?e.id,
                error = ?e.error,
                reason = ?e.reason,
                "Bulk doc error"
            );
        }
    }
    batch.clear();
    Ok(())
}

async fn soft_delete_missing(
    db: &CouchClient,
    existing: &HashMap<String, serde_json::Value>,
    seen_paths: &HashMap<String, bool>,
    replication: Option<&ReplicationHandle>,
) -> anyhow::Result<u64> {
    let mut batch: Vec<serde_json::Value> = Vec::new();
    let mut count = 0u64;

    for (path, doc) in existing {
        let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("active");
        if status == "deleted" {
            continue;
        }
        if seen_paths.contains_key(path) {
            continue;
        }

        // File no longer exists — soft delete
        let file_id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut updated = doc.clone();
        updated["status"] = serde_json::Value::String("deleted".to_string());
        updated["deleted_at"] = serde_json::Value::String(Utc::now().to_rfc3339());
        batch.push(updated);
        count += 1;

        // Emit deletion event for replication subsystem
        if let Some(rh) = replication {
            rh.send(FileEvent::Deleted { file_id });
        }

        if batch.len() >= BATCH_SIZE {
            flush_batch(db, &mut batch).await?;
        }
    }

    if !batch.is_empty() {
        flush_batch(db, &mut batch).await?;
    }

    Ok(count)
}
