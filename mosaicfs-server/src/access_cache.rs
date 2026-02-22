use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use tracing::{debug, info};

use crate::couchdb::CouchClient;

/// Materialized access cache: file_id → last access time.
pub struct AccessCache {
    accesses: RwLock<HashMap<String, DateTime<Utc>>>,
}

impl AccessCache {
    pub fn new() -> Self {
        Self {
            accesses: RwLock::new(HashMap::new()),
        }
    }

    /// Build cache from all access::* docs in CouchDB.
    pub async fn build(&self, db: &CouchClient) -> anyhow::Result<()> {
        let mut map: HashMap<String, DateTime<Utc>> = HashMap::new();

        let docs = db.all_docs_by_prefix("access::", true).await?;
        for row in &docs.rows {
            if let Some(doc) = &row.doc {
                if let (Some(file_id), Some(last_access)) = (
                    doc.get("file_id").and_then(|v| v.as_str()),
                    doc.get("last_access").and_then(|v| v.as_str()),
                ) {
                    if let Ok(ts) = last_access.parse::<DateTime<Utc>>() {
                        map.insert(file_id.to_string(), ts);
                    }
                }
            }
        }

        let count = map.len();
        *self.accesses.write().unwrap() = map;
        info!(entries = count, "Access cache built");
        Ok(())
    }

    pub fn last_access(&self, file_id: &str) -> Option<DateTime<Utc>> {
        self.accesses.read().unwrap().get(file_id).copied()
    }

    /// Update cache from a flushed access record (called after AccessTracker flush).
    pub fn record_access(&self, file_id: &str, timestamp: DateTime<Utc>) {
        self.accesses.write().unwrap().insert(file_id.to_string(), timestamp);
    }

    /// Handle a CouchDB changes feed document.
    pub fn handle_change(&self, doc: &serde_json::Value, deleted: bool) {
        let id = match doc.get("_id").or_else(|| doc.get("id")).and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return,
        };

        if !id.starts_with("access::") {
            return;
        }

        if deleted {
            // Find and remove the entry by scanning for the file_id
            if let Some(file_id) = doc.get("file_id").and_then(|v| v.as_str()) {
                self.accesses.write().unwrap().remove(file_id);
                debug!(file_id, "Access cache: removed");
            }
        } else if let (Some(file_id), Some(last_access)) = (
            doc.get("file_id").and_then(|v| v.as_str()),
            doc.get("last_access").and_then(|v| v.as_str()),
        ) {
            if let Ok(ts) = last_access.parse::<DateTime<Utc>>() {
                self.accesses.write().unwrap().insert(file_id.to_string(), ts);
                debug!(file_id, "Access cache: updated");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_cache_basic() {
        let cache = AccessCache::new();
        assert!(cache.last_access("file::abc").is_none());

        let now = Utc::now();
        cache.record_access("file::abc", now);
        assert_eq!(cache.last_access("file::abc"), Some(now));
    }

    #[test]
    fn test_handle_change_update() {
        let cache = AccessCache::new();
        let doc = serde_json::json!({
            "_id": "access::abc",
            "file_id": "file::abc",
            "last_access": "2026-01-15T10:00:00Z",
        });
        cache.handle_change(&doc, false);
        assert!(cache.last_access("file::abc").is_some());
    }

    #[test]
    fn test_handle_change_delete() {
        let cache = AccessCache::new();
        let now = Utc::now();
        cache.record_access("file::abc", now);

        let doc = serde_json::json!({
            "_id": "access::abc",
            "file_id": "file::abc",
        });
        cache.handle_change(&doc, true);
        assert!(cache.last_access("file::abc").is_none());
    }
}
