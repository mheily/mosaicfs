use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::access_cache::AccessCache;
use crate::couchdb::CouchClient;
use crate::handlers::replication::{RestoreJob, RestoreJobStore};
use crate::label_cache::LabelCache;
use crate::readdir_cache::ReaddirCache;

/// Pending access record (file_id -> last_access timestamp in DB, if known)
pub struct AccessTracker {
    /// file_id -> timestamp of pending access
    pub pending: HashMap<String, chrono::DateTime<chrono::Utc>>,
    /// file_id -> last known access time in DB (for debounce)
    pub last_written: HashMap<String, chrono::DateTime<chrono::Utc>>,
}

impl AccessTracker {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            last_written: HashMap::new(),
        }
    }

    /// Record an access. Debounces: only queues write if last access >1 hour ago.
    pub fn record(&mut self, file_id: &str) {
        let now = chrono::Utc::now();
        let debounce = chrono::Duration::hours(1);

        if let Some(last) = self.last_written.get(file_id) {
            if now - *last < debounce {
                return; // too recent, skip
            }
        }

        self.pending.insert(file_id.to_string(), now);
    }

    /// Take all pending accesses for flushing
    pub fn take_pending(&mut self) -> HashMap<String, chrono::DateTime<chrono::Utc>> {
        let taken = std::mem::take(&mut self.pending);
        // Update last_written with what we're about to flush
        for (id, ts) in &taken {
            self.last_written.insert(id.clone(), *ts);
        }
        taken
    }
}

/// Shared application state
pub struct AppState {
    pub db: CouchClient,
    pub jwt_secret: Vec<u8>,
    pub couchdb_url: String,
    pub couchdb_user: String,
    pub couchdb_password: String,
    /// Invalidated JWT token IDs
    pub revoked_tokens: Mutex<Vec<String>>,
    /// Rate limiter: key -> (count, window_start)
    pub login_attempts: Mutex<HashMap<String, (u32, Instant)>>,
    /// Access tracking with debounce and batching
    pub access_tracker: Mutex<AccessTracker>,
    /// Materialized label cache
    pub label_cache: Arc<LabelCache>,
    /// Materialized access cache
    pub access_cache: Arc<AccessCache>,
    /// Short-lived readdir cache
    pub readdir_cache: Arc<ReaddirCache>,
    /// In-memory restore job store
    pub restore_jobs: RestoreJobStore,
    /// Developer mode enables destructive endpoints (DELETE /api/system/data)
    pub developer_mode: bool,
    /// Server startup time for uptime calculation
    pub started_at: Instant,
}

impl AppState {
    pub fn new(
        db: CouchClient,
        jwt_secret: Vec<u8>,
        label_cache: Arc<LabelCache>,
        access_cache: Arc<AccessCache>,
        developer_mode: bool,
    ) -> Self {
        let couchdb_url = std::env::var("COUCHDB_URL").unwrap_or_else(|_| "http://localhost:5984".to_string());
        let couchdb_user = std::env::var("COUCHDB_USER").unwrap_or_else(|_| "admin".to_string());
        let couchdb_password = std::env::var("COUCHDB_PASSWORD").unwrap_or_default();
        Self {
            db,
            jwt_secret,
            couchdb_url,
            couchdb_user,
            couchdb_password,
            revoked_tokens: Mutex::new(Vec::new()),
            login_attempts: Mutex::new(HashMap::new()),
            access_tracker: Mutex::new(AccessTracker::new()),
            label_cache,
            access_cache,
            readdir_cache: Arc::new(ReaddirCache::new()),
            restore_jobs: Arc::new(Mutex::new(HashMap::new())),
            developer_mode,
            started_at: Instant::now(),
        }
    }

    /// Record a file access (debounced)
    pub fn record_access(&self, file_id: &str) {
        if let Ok(mut tracker) = self.access_tracker.lock() {
            tracker.record(file_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_tracker_debounce() {
        let mut tracker = AccessTracker::new();

        // First access should be recorded
        tracker.record("file::abc");
        assert_eq!(tracker.pending.len(), 1);

        // Take pending
        let taken = tracker.take_pending();
        assert_eq!(taken.len(), 1);
        assert!(tracker.pending.is_empty());

        // Second access within 1 hour should be debounced
        tracker.record("file::abc");
        assert!(tracker.pending.is_empty()); // debounced

        // Different file should work
        tracker.record("file::def");
        assert_eq!(tracker.pending.len(), 1);
    }
}
