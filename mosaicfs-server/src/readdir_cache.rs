use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::readdir::ReaddirEntry;

const DEFAULT_TTL_SECS: u64 = 5;

/// Cache key: (virtual_path, directory_doc_rev)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    path: String,
    rev: String,
}

struct CacheEntry {
    entries: Vec<ReaddirEntry>,
    inserted_at: Instant,
}

/// Short-lived readdir cache to avoid re-evaluating mounts on every request.
pub struct ReaddirCache {
    cache: RwLock<HashMap<CacheKey, CacheEntry>>,
    ttl: Duration,
}

impl ReaddirCache {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
        }
    }

    pub fn get(&self, path: &str, rev: &str) -> Option<Vec<ReaddirEntry>> {
        let key = CacheKey {
            path: path.to_string(),
            rev: rev.to_string(),
        };
        let cache = self.cache.read().unwrap();
        if let Some(entry) = cache.get(&key) {
            if entry.inserted_at.elapsed() < self.ttl {
                return Some(entry.entries.clone());
            }
        }
        None
    }

    pub fn put(&self, path: &str, rev: &str, entries: Vec<ReaddirEntry>) {
        let key = CacheKey {
            path: path.to_string(),
            rev: rev.to_string(),
        };
        let mut cache = self.cache.write().unwrap();
        cache.insert(
            key,
            CacheEntry {
                entries,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Invalidate entries for a given path (called from changes feed).
    pub fn invalidate(&self, path: &str) {
        let mut cache = self.cache.write().unwrap();
        cache.retain(|k, _| k.path != path);
    }

    /// Evict expired entries.
    pub fn evict_expired(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.retain(|_, v| v.inserted_at.elapsed() < self.ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn dummy_entry() -> ReaddirEntry {
        ReaddirEntry {
            name: "test.txt".to_string(),
            file_id: "file::abc".to_string(),
            size: 100,
            mtime: Utc::now(),
            mime_type: None,
            source_node_id: "n1".to_string(),
            source_export_path: "/test.txt".to_string(),
            mount_id: "m1".to_string(),
        }
    }

    #[test]
    fn test_cache_hit() {
        let cache = ReaddirCache::new();
        cache.put("/", "1-abc", vec![dummy_entry()]);
        assert!(cache.get("/", "1-abc").is_some());
        assert_eq!(cache.get("/", "1-abc").unwrap().len(), 1);
    }

    #[test]
    fn test_cache_miss_wrong_rev() {
        let cache = ReaddirCache::new();
        cache.put("/", "1-abc", vec![dummy_entry()]);
        assert!(cache.get("/", "2-def").is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = ReaddirCache::new();
        cache.put("/", "1-abc", vec![dummy_entry()]);
        cache.invalidate("/");
        assert!(cache.get("/", "1-abc").is_none());
    }
}
