use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use tracing::{debug, info, warn};

use crate::couchdb::CouchClient;

/// Materialized label cache: file_uuid → set of labels (union of assignments + rules).
pub struct LabelCache {
    labels: RwLock<HashMap<String, HashSet<String>>>,
}

impl LabelCache {
    pub fn new() -> Self {
        Self {
            labels: RwLock::new(HashMap::new()),
        }
    }

    /// Build the cache from CouchDB: load all label_assignment and enabled label_rule docs.
    pub async fn build(&self, db: &CouchClient) -> anyhow::Result<()> {
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();

        // Load label assignments
        let assignments = db.all_docs_by_prefix("label_assignment::", true).await?;
        for row in &assignments.rows {
            if let Some(doc) = &row.doc {
                if let (Some(file_id), Some(labels)) = (
                    doc.get("file_id").and_then(|v| v.as_str()),
                    doc.get("labels").and_then(|v| v.as_array()),
                ) {
                    let uuid = file_id.strip_prefix("file::").unwrap_or(file_id);
                    let label_set = map.entry(uuid.to_string()).or_default();
                    for l in labels {
                        if let Some(s) = l.as_str() {
                            label_set.insert(s.to_string());
                        }
                    }
                }
            }
        }

        // Load enabled label rules and apply to matching files
        let rules = db.all_docs_by_prefix("label_rule::", true).await?;
        let enabled_rules: Vec<&serde_json::Value> = rules
            .rows
            .iter()
            .filter_map(|r| r.doc.as_ref())
            .filter(|d| d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false))
            .collect();

        if !enabled_rules.is_empty() {
            // Load all files to match rules against
            let files = db.all_docs_by_prefix("file::", true).await?;
            for file_row in &files.rows {
                if let Some(file_doc) = &file_row.doc {
                    let file_uuid = file_row.id.strip_prefix("file::").unwrap_or(&file_row.id);
                    let node_id = file_doc
                        .get("source")
                        .and_then(|s| s.get("node_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let export_path = file_doc
                        .get("source")
                        .and_then(|s| s.get("export_path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    for rule in &enabled_rules {
                        if rule_matches_file(rule, node_id, export_path) {
                            if let Some(labels) = rule.get("labels").and_then(|v| v.as_array()) {
                                let label_set = map.entry(file_uuid.to_string()).or_default();
                                for l in labels {
                                    if let Some(s) = l.as_str() {
                                        label_set.insert(s.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let count = map.len();
        *self.labels.write().unwrap() = map;
        info!(files = count, "Label cache built");
        Ok(())
    }

    pub fn has_label(&self, file_uuid: &str, label: &str) -> bool {
        self.labels
            .read()
            .unwrap()
            .get(file_uuid)
            .map(|s| s.contains(label))
            .unwrap_or(false)
    }

    pub fn get_labels(&self, file_uuid: &str) -> HashSet<String> {
        self.labels
            .read()
            .unwrap()
            .get(file_uuid)
            .cloned()
            .unwrap_or_default()
    }

    /// Handle a CouchDB changes feed document.
    pub async fn handle_change(&self, doc: &serde_json::Value, deleted: bool, db: &CouchClient) {
        let id = match doc.get("_id").or_else(|| doc.get("id")).and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return,
        };

        if id.starts_with("label_assignment::") {
            if deleted {
                let file_uuid = id.strip_prefix("label_assignment::").unwrap_or("");
                self.labels.write().unwrap().remove(file_uuid);
                debug!(file_uuid, "Label cache: removed assignment");
            } else if let (Some(file_id), Some(labels)) = (
                doc.get("file_id").and_then(|v| v.as_str()),
                doc.get("labels").and_then(|v| v.as_array()),
            ) {
                let uuid = file_id.strip_prefix("file::").unwrap_or(file_id);
                let new_labels: HashSet<String> = labels
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                // Merge with existing (rule-derived labels remain)
                let mut map = self.labels.write().unwrap();
                let entry = map.entry(uuid.to_string()).or_default();
                for l in new_labels {
                    entry.insert(l);
                }
                debug!(file_uuid = uuid, "Label cache: updated assignment");
            }
        } else if id.starts_with("label_rule::") {
            // For rule changes, do a full rebuild (rules affect many files)
            debug!("Label cache: rule changed, rebuilding");
            if let Err(e) = self.build(db).await {
                warn!(error = %e, "Label cache rebuild failed");
            }
        }
    }
}

fn rule_matches_file(rule: &serde_json::Value, node_id: &str, export_path: &str) -> bool {
    let rule_node = rule.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
    if rule_node != node_id {
        return false;
    }
    let prefix = rule.get("path_prefix").and_then(|v| v.as_str()).unwrap_or("");
    export_path.starts_with(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_cache_basic() {
        let cache = LabelCache::new();
        assert!(!cache.has_label("abc", "work"));

        {
            let mut map = cache.labels.write().unwrap();
            map.entry("abc".to_string())
                .or_default()
                .insert("work".to_string());
        }

        assert!(cache.has_label("abc", "work"));
        assert!(!cache.has_label("abc", "personal"));
        assert_eq!(cache.get_labels("abc"), HashSet::from(["work".to_string()]));
        assert!(cache.get_labels("xyz").is_empty());
    }

    #[test]
    fn test_rule_matches_file() {
        let rule = serde_json::json!({
            "node_id": "node-1",
            "path_prefix": "/docs/",
        });
        assert!(rule_matches_file(&rule, "node-1", "/docs/report.pdf"));
        assert!(!rule_matches_file(&rule, "node-2", "/docs/report.pdf"));
        assert!(!rule_matches_file(&rule, "node-1", "/photos/pic.jpg"));
    }
}
