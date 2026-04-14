//! Canonical readdir evaluator.
//!
//! Resolves a virtual directory's mount entries against file documents and
//! returns the merged file list. Both the FUSE layer and the server's REST
//! API call into this module — the former wraps the result into FUSE dirents,
//! the latter into JSON (see `mosaicfs-server/src/readdir.rs`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use mosaicfs_common::couchdb::CouchClient;
use mosaicfs_common::documents::*;
use mosaicfs_common::steps::{self, StepContext};

/// A file entry produced by readdir evaluation.
#[derive(Debug, Clone)]
pub struct ReaddirEntry {
    pub name: String,
    pub file_id: String,
    pub inode: u64,
    pub size: u64,
    pub mtime: DateTime<Utc>,
    pub mime_type: Option<String>,
    pub source_node_id: String,
    pub source_export_path: String,
    pub mount_id: String,
}

/// A directory entry in readdir results (used by FUSE).
#[derive(Debug, Clone)]
pub struct ReaddirDirEntry {
    pub name: String,
    pub inode: u64,
    pub virtual_path: String,
}

/// Default `StepContext` implementation for callers that do not maintain
/// their own label/access caches (notably the FUSE layer during early
/// evaluation).
pub struct VfsStepContext {
    pub labels: HashMap<String, Vec<String>>,
    pub accesses: HashMap<String, DateTime<Utc>>,
    pub replicas: HashMap<String, Vec<(String, String)>>,
    pub annotations: HashMap<String, Vec<String>>,
}

impl VfsStepContext {
    pub fn empty() -> Self {
        Self {
            labels: HashMap::new(),
            accesses: HashMap::new(),
            replicas: HashMap::new(),
            annotations: HashMap::new(),
        }
    }
}

impl StepContext for VfsStepContext {
    fn has_label(&self, file_uuid: &str, label: &str) -> bool {
        self.labels
            .get(file_uuid)
            .map(|v| v.iter().any(|l| l == label))
            .unwrap_or(false)
    }

    fn last_access(&self, file_id: &str) -> Option<DateTime<Utc>> {
        self.accesses.get(file_id).copied()
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

/// Evaluate readdir for a virtual directory given its mounts.
///
/// `label_files` resolves a label name to the file ids it applies to — the
/// FUSE layer can pass a no-op while the server uses its `LabelCache`.
pub async fn evaluate_readdir<C, F>(
    db: &CouchClient,
    mounts: &[MountEntry],
    inherited_steps: &[Step],
    child_dirs: &[String],
    ctx: &C,
    label_files: F,
) -> Result<Vec<ReaddirEntry>, anyhow::Error>
where
    C: StepContext,
    F: Fn(&str) -> Vec<String>,
{
    let mut result_entries: HashMap<String, ReaddirEntry> = HashMap::new();
    let mut conflict_policies: HashMap<String, ConflictPolicy> = HashMap::new();

    for mount in mounts {
        let files = query_mount_files(db, &mount.source, &label_files).await?;

        for (file_id, file_doc) in &files {
            let mut all_steps: Vec<Step> = inherited_steps.to_vec();
            all_steps.extend(mount.steps.clone());

            // With ancestor-inherited steps acting as an explicit filter,
            // unmatched files should be excluded; otherwise use the mount's
            // own default.
            let effective_default = if inherited_steps.is_empty() {
                mount.default_result.clone()
            } else {
                StepResult::Exclude
            };
            let step_result =
                steps::evaluate_steps(&all_steps, file_doc, file_id, &effective_default, ctx);

            if step_result != StepResult::Include {
                continue;
            }

            let mapped_name = map_filename(&mount.strategy, &mount.source, file_doc);

            if child_dirs.iter().any(|d| d == &mapped_name) {
                continue;
            }

            let entry = ReaddirEntry {
                name: mapped_name.clone(),
                file_id: file_id.clone(),
                inode: file_doc.inode,
                size: file_doc.size,
                mtime: file_doc.mtime,
                mime_type: file_doc.mime_type.clone(),
                source_node_id: file_doc.source.node_id.clone(),
                source_export_path: file_doc.source.export_path.clone(),
                mount_id: mount.mount_id.clone(),
            };

            if let Some(existing) = result_entries.get(&mapped_name) {
                let policy = conflict_policies
                    .get(&mapped_name)
                    .unwrap_or(&mount.conflict_policy);
                match policy {
                    ConflictPolicy::LastWriteWins => {
                        if entry.mtime > existing.mtime {
                            result_entries.insert(mapped_name.clone(), entry);
                            conflict_policies
                                .insert(mapped_name, mount.conflict_policy.clone());
                        }
                    }
                    ConflictPolicy::SuffixNodeId => {
                        let (stem, ext) = split_extension(&mapped_name);
                        let suffixed = if ext.is_empty() {
                            format!("{} ({})", stem, entry.source_node_id)
                        } else {
                            format!("{} ({}).{}", stem, entry.source_node_id, ext)
                        };
                        let mut suffixed_entry = entry;
                        suffixed_entry.name = suffixed.clone();
                        result_entries.insert(suffixed, suffixed_entry);
                    }
                }
            } else {
                conflict_policies.insert(mapped_name.clone(), mount.conflict_policy.clone());
                result_entries.insert(mapped_name, entry);
            }
        }
    }

    let mut entries: Vec<ReaddirEntry> = result_entries.into_values().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Collect inherited steps from ancestor directories (root → parent).
pub async fn collect_inherited_steps(
    db: &CouchClient,
    virtual_path: &str,
) -> Result<Vec<Step>, anyhow::Error> {
    if virtual_path == "/" {
        return Ok(vec![]);
    }

    let mut inherited = Vec::new();
    let ancestors = ancestor_paths(virtual_path);

    for ancestor_path in &ancestors {
        let doc_id = dir_id_for(ancestor_path);
        if let Ok(doc) = db.get_document(&doc_id).await {
            let enforce = doc
                .get("enforce_steps_on_children")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if enforce {
                if let Some(mounts) = doc.get("mounts").and_then(|v| v.as_array()) {
                    for mount in mounts {
                        if let Some(mount_steps) = mount.get("steps").and_then(|v| v.as_array()) {
                            for s in mount_steps {
                                if let Ok(step) = serde_json::from_value::<Step>(s.clone()) {
                                    inherited.push(step);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(inherited)
}

/// Compute the CouchDB document ID for a virtual directory path.
pub fn dir_id_for(path: &str) -> String {
    if path == "/" {
        "dir::root".to_string()
    } else {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(path.as_bytes());
        format!("dir::{}", hex::encode(hash))
    }
}

fn ancestor_paths(path: &str) -> Vec<String> {
    let mut ancestors = vec!["/".to_string()];
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').collect();
    for i in 0..parts.len().saturating_sub(1) {
        let p = format!("/{}", parts[..=i].join("/"));
        if p != "/" {
            ancestors.push(p);
        }
    }
    ancestors
}

fn map_filename(
    strategy: &MountStrategy,
    source: &MountSource,
    file_doc: &FileDocument,
) -> String {
    match strategy {
        MountStrategy::Flatten => file_doc.name.clone(),
        MountStrategy::PrefixReplace => match source {
            MountSource::Node { export_path, .. } => {
                let rel = file_doc
                    .source
                    .export_path
                    .strip_prefix(export_path)
                    .unwrap_or(&file_doc.source.export_path)
                    .trim_start_matches('/');
                if rel.is_empty() {
                    file_doc.name.clone()
                } else {
                    match rel.split_once('/') {
                        Some((first, _)) => first.to_string(),
                        None => rel.to_string(),
                    }
                }
            }
            MountSource::Federated { .. } | MountSource::Label { .. } => file_doc.name.clone(),
        },
    }
}

async fn query_mount_files<F>(
    db: &CouchClient,
    source: &MountSource,
    label_files: &F,
) -> Result<Vec<(String, FileDocument)>, anyhow::Error>
where
    F: Fn(&str) -> Vec<String>,
{
    match source {
        MountSource::Node { node_id, export_path } => {
            let selector = serde_json::json!({
                "type": "file",
                "status": "active",
                "source.node_id": node_id,
                "source.export_parent": {
                    "$regex": format!("^{}", regex::escape(export_path.trim_end_matches('/')))
                }
            });

            let resp = db.find(selector).await?;
            let mut results = Vec::new();
            for doc in resp.docs {
                let id = doc
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Ok(file_doc) = serde_json::from_value::<FileDocument>(doc) {
                    results.push((id, file_doc));
                }
            }
            Ok(results)
        }
        MountSource::Label { label } => {
            let ids = label_files(label);
            let mut results = Vec::new();
            for file_id in ids {
                if let Ok(doc) = db.get_document(&file_id).await {
                    if doc.get("status").and_then(|v| v.as_str()) == Some("active") {
                        if let Ok(file_doc) = serde_json::from_value::<FileDocument>(doc) {
                            results.push((file_id, file_doc));
                        }
                    }
                }
            }
            Ok(results)
        }
        MountSource::Federated { .. } => Ok(vec![]),
    }
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(pos) if pos > 0 => (&name[..pos], &name[pos + 1..]),
        _ => (name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ancestor_paths() {
        assert_eq!(ancestor_paths("/"), vec!["/"]);
        assert_eq!(ancestor_paths("/a"), vec!["/"]);
        assert_eq!(ancestor_paths("/a/b"), vec!["/", "/a"]);
        assert_eq!(ancestor_paths("/a/b/c"), vec!["/", "/a", "/a/b"]);
    }

    #[test]
    fn test_split_extension() {
        assert_eq!(split_extension("report.pdf"), ("report", "pdf"));
        assert_eq!(split_extension("archive.tar.gz"), ("archive.tar", "gz"));
        assert_eq!(split_extension("readme"), ("readme", ""));
        assert_eq!(split_extension(".hidden"), (".hidden", ""));
    }
}
