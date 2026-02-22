use std::collections::HashMap;
use chrono::{DateTime, Utc};
use mosaicfs_common::documents::{
    ConflictPolicy, FileDocument, MountEntry, MountSource, MountStrategy, Step, StepResult,
};
use mosaicfs_common::steps::{self, StepContext};

use crate::access_cache::AccessCache;
use crate::couchdb::CouchClient;
use crate::label_cache::LabelCache;

/// A file entry produced by readdir evaluation.
#[derive(Debug, Clone)]
pub struct ReaddirEntry {
    pub name: String,
    pub file_id: String,
    pub size: u64,
    pub mtime: DateTime<Utc>,
    pub mime_type: Option<String>,
    pub source_node_id: String,
    pub source_export_path: String,
    pub mount_id: String,
}

/// Context bridging label/access caches + CouchDB for replica/annotation lookups.
struct EvalContext<'a> {
    label_cache: &'a LabelCache,
    access_cache: &'a AccessCache,
    replicas: HashMap<String, Vec<(String, String)>>, // file_uuid -> [(target, status)]
    annotations: HashMap<String, Vec<String>>,         // file_uuid -> [plugin_name]
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

/// Evaluate readdir for a virtual directory, returning file entries and subdirectory names.
///
/// `inherited_steps` = steps collected from ancestors (root→parent) when `enforce_steps_on_children` is set.
pub async fn evaluate_readdir(
    db: &CouchClient,
    label_cache: &LabelCache,
    access_cache: &AccessCache,
    mounts: &[MountEntry],
    inherited_steps: &[Step],
    child_dirs: &[String], // child virtual_directory names
) -> Result<Vec<ReaddirEntry>, anyhow::Error> {
    // Collect all file_uuids that need replica/annotation lookups
    // We do this lazily — first evaluate without replicas/annotations, which covers most ops
    let ctx = EvalContext {
        label_cache,
        access_cache,
        replicas: HashMap::new(),
        annotations: HashMap::new(),
    };

    let mut result_entries: HashMap<String, ReaddirEntry> = HashMap::new(); // name -> entry
    let mut conflict_policies: HashMap<String, ConflictPolicy> = HashMap::new(); // name -> policy

    for mount in mounts {
        let files = query_mount_files(db, &mount.source).await?;

        for (file_id, file_doc, raw_doc) in &files {
            // Build combined step list: inherited + mount steps
            let mut all_steps: Vec<Step> = inherited_steps.to_vec();
            all_steps.extend(mount.steps.clone());

            let step_result =
                steps::evaluate_steps(&all_steps, file_doc, file_id, &mount.default_result, &ctx);

            if step_result != StepResult::Include {
                continue;
            }

            // Apply mapping strategy to get the virtual file name
            let mapped_name = match &mount.strategy {
                MountStrategy::Flatten => file_doc.name.clone(),
                MountStrategy::PrefixReplace => {
                    // Strip the source prefix from export_path to get relative path,
                    // then use just the filename component
                    match &mount.source {
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
                                // For prefix_replace, use the relative path (preserving subdirs)
                                // but for readdir we only show the first component
                                match rel.split_once('/') {
                                    Some((first, _)) => first.to_string(),
                                    None => rel.to_string(),
                                }
                            }
                        }
                        MountSource::Federated { .. } => file_doc.name.clone(),
                    }
                }
            };

            // Skip if the name collides with a child directory
            if child_dirs.iter().any(|d| d == &mapped_name) {
                continue;
            }

            let entry = ReaddirEntry {
                name: mapped_name.clone(),
                file_id: file_id.clone(),
                size: file_doc.size,
                mtime: file_doc.mtime,
                mime_type: file_doc.mime_type.clone(),
                source_node_id: file_doc.source.node_id.clone(),
                source_export_path: file_doc.source.export_path.clone(),
                mount_id: mount.mount_id.clone(),
            };

            // Handle name conflicts
            if let Some(existing) = result_entries.get(&mapped_name) {
                let policy = conflict_policies
                    .get(&mapped_name)
                    .unwrap_or(&mount.conflict_policy);
                match policy {
                    ConflictPolicy::LastWriteWins => {
                        // Keep the one with the more recent mtime
                        if entry.mtime > existing.mtime {
                            result_entries.insert(mapped_name.clone(), entry);
                            conflict_policies.insert(mapped_name, mount.conflict_policy.clone());
                        }
                    }
                    ConflictPolicy::SuffixNodeId => {
                        // Rename new entry with node_id suffix
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

/// Collect inherited steps by walking the ancestor chain from root to the given path.
pub async fn collect_inherited_steps(
    db: &CouchClient,
    virtual_path: &str,
) -> Result<Vec<Step>, anyhow::Error> {
    if virtual_path == "/" {
        return Ok(vec![]);
    }

    let mut steps = Vec::new();
    let ancestors = ancestor_paths(virtual_path);

    for ancestor_path in &ancestors {
        let doc_id = crate::handlers::vfs::dir_id_for(ancestor_path);
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
                                    steps.push(step);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(steps)
}

/// Return ancestor paths from root to parent (excluding the path itself).
/// E.g., "/a/b/c" → ["/", "/a", "/a/b"]
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

/// Query files matching a mount source.
async fn query_mount_files(
    db: &CouchClient,
    source: &MountSource,
) -> Result<Vec<(String, FileDocument, serde_json::Value)>, anyhow::Error> {
    match source {
        MountSource::Node {
            node_id,
            export_path,
        } => {
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
                let id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Ok(file_doc) = serde_json::from_value::<FileDocument>(doc.clone()) {
                    results.push((id, file_doc, doc));
                }
            }
            Ok(results)
        }
        MountSource::Federated { .. } => {
            // Federated imports not yet implemented
            Ok(vec![])
        }
    }
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(pos) if pos > 0 => (&name[..pos], &name[pos + 1..]),
        _ => (name, ""),
    }
}

/// Convert ReaddirEntry list to JSON for API response.
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

    #[test]
    fn test_entries_to_json() {
        let entries = vec![ReaddirEntry {
            name: "test.txt".to_string(),
            file_id: "file::abc".to_string(),
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
