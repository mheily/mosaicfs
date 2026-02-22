//! Full-file and block-mode cache backed by SQLite.
//!
//! Cache directory: `/var/lib/mosaicfs/cache/`
//! SQLite index: `cache/index.db`
//! File layout: `cache/{shard_prefix}/{file_uuid}` (shard = first 2 UUID chars)
//! Staging: `cache/tmp/{uuid}` (atomic rename on completion)

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, info, warn};

use crate::block_map::BlockMap;

const FULL_FILE_SIZE_THRESHOLD: u64 = 50 * 1024 * 1024; // 50 MB
const DEFAULT_BLOCK_SIZE: u64 = 4 * 1024 * 1024; // 4 MB blocks
const FRAGMENTATION_LIMIT: usize = 1000;

/// A single cache entry record.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub cache_key: String,   // file_uuid
    pub file_id: String,     // full file_id (file::uuid)
    pub mtime: String,       // ISO 8601
    pub size_on_record: u64, // expected file size
    pub block_size: u64,
    pub block_map: Option<Vec<u8>>, // serialized BlockMap (None = full file mode)
    pub cached_bytes: u64,
    pub last_access: String, // ISO 8601
    pub source: String,      // "local", "remote:{node_id}", "plugin:{name}"
}

pub struct FileCache {
    pub cache_dir: PathBuf,
    db: Connection,
}

impl FileCache {
    /// Open or create the cache at the given directory.
    pub fn open(cache_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(cache_dir)?;
        std::fs::create_dir_all(cache_dir.join("tmp"))?;

        let db_path = cache_dir.join("index.db");
        let db = Connection::open(&db_path)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache_entries (
                cache_key TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                mtime TEXT NOT NULL,
                size_on_record INTEGER NOT NULL,
                block_size INTEGER NOT NULL DEFAULT 4194304,
                block_map BLOB,
                cached_bytes INTEGER NOT NULL DEFAULT 0,
                last_access TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'local'
            );
            CREATE INDEX IF NOT EXISTS idx_cache_last_access ON cache_entries(last_access);
            CREATE INDEX IF NOT EXISTS idx_cache_file_id ON cache_entries(file_id);",
        )?;

        Ok(Self {
            cache_dir: cache_dir.to_path_buf(),
            db,
        })
    }

    /// Determine if a file should use block mode (large file) or full-file mode.
    pub fn should_use_block_mode(size: u64) -> bool {
        size > FULL_FILE_SIZE_THRESHOLD
    }

    /// Get the shard directory path for a cache key (first 2 chars of UUID).
    fn shard_path(&self, cache_key: &str) -> PathBuf {
        let prefix = &cache_key[..2.min(cache_key.len())];
        self.cache_dir.join(prefix)
    }

    /// Get the file path for a cached entry.
    pub fn entry_path(&self, cache_key: &str) -> PathBuf {
        self.shard_path(cache_key).join(cache_key)
    }

    /// Get a staging path for a download in progress.
    pub fn staging_path(&self) -> PathBuf {
        self.cache_dir
            .join("tmp")
            .join(uuid::Uuid::new_v4().to_string())
    }

    /// Look up a cache entry.
    pub fn get_entry(&self, cache_key: &str) -> anyhow::Result<Option<CacheEntry>> {
        let result = self.db.query_row(
            "SELECT cache_key, file_id, mtime, size_on_record, block_size, block_map,
                    cached_bytes, last_access, source
             FROM cache_entries WHERE cache_key = ?1",
            params![cache_key],
            |row| {
                Ok(CacheEntry {
                    cache_key: row.get(0)?,
                    file_id: row.get(1)?,
                    mtime: row.get(2)?,
                    size_on_record: row.get(3)?,
                    block_size: row.get(4)?,
                    block_map: row.get(5)?,
                    cached_bytes: row.get(6)?,
                    last_access: row.get(7)?,
                    source: row.get(8)?,
                })
            },
        ).optional()?;
        Ok(result)
    }

    /// Check if a cached file is stale (mtime or size mismatch).
    pub fn is_stale(&self, cache_key: &str, current_mtime: &str, current_size: u64) -> bool {
        match self.get_entry(cache_key) {
            Ok(Some(entry)) => entry.mtime != current_mtime || entry.size_on_record != current_size,
            _ => true, // Missing = stale
        }
    }

    /// Store a full-file cache entry. The file must already exist at `entry_path`.
    pub fn store_full_file(
        &self,
        cache_key: &str,
        file_id: &str,
        mtime: &str,
        size: u64,
        source: &str,
    ) -> anyhow::Result<()> {
        // Ensure shard directory exists
        let shard = self.shard_path(cache_key);
        std::fs::create_dir_all(&shard)?;

        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT OR REPLACE INTO cache_entries
             (cache_key, file_id, mtime, size_on_record, block_size, block_map,
              cached_bytes, last_access, source)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8)",
            params![cache_key, file_id, mtime, size, DEFAULT_BLOCK_SIZE, size, now, source],
        )?;
        Ok(())
    }

    /// Create or update a block-mode cache entry.
    pub fn store_block_entry(
        &self,
        cache_key: &str,
        file_id: &str,
        mtime: &str,
        size: u64,
        block_map: &BlockMap,
        source: &str,
    ) -> anyhow::Result<()> {
        let shard = self.shard_path(cache_key);
        std::fs::create_dir_all(&shard)?;

        let bm_bytes = block_map.to_bytes();
        let cached = block_map.cached_bytes();
        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT OR REPLACE INTO cache_entries
             (cache_key, file_id, mtime, size_on_record, block_size, block_map,
              cached_bytes, last_access, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                cache_key,
                file_id,
                mtime,
                size,
                DEFAULT_BLOCK_SIZE,
                bm_bytes,
                cached,
                now,
                source
            ],
        )?;
        Ok(())
    }

    /// Update last_access for an entry.
    pub fn touch(&self, cache_key: &str) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE cache_entries SET last_access = ?1 WHERE cache_key = ?2",
            params![now, cache_key],
        )?;
        Ok(())
    }

    /// Get total cached bytes across all entries.
    pub fn total_cached_bytes(&self) -> anyhow::Result<u64> {
        let total: u64 = self.db.query_row(
            "SELECT COALESCE(SUM(cached_bytes), 0) FROM cache_entries",
            [],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    /// Evict entries by LRU until total_cached_bytes <= cap and free space >= min_free.
    /// Returns number of entries evicted.
    pub fn evict_lru(&self, cap_bytes: u64, min_free_bytes: u64) -> anyhow::Result<u64> {
        let mut evicted = 0u64;

        loop {
            let total = self.total_cached_bytes()?;
            let free = available_disk_space(&self.cache_dir);

            if total <= cap_bytes && free >= min_free_bytes {
                break;
            }

            // Find the LRU entry
            let entry: Option<(String, u64)> = self
                .db
                .query_row(
                    "SELECT cache_key, cached_bytes FROM cache_entries
                     ORDER BY last_access ASC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            match entry {
                Some((key, _)) => {
                    self.remove_entry(&key)?;
                    evicted += 1;
                }
                None => break, // No entries left
            }
        }

        if evicted > 0 {
            info!(evicted, "Cache eviction completed");
        }
        Ok(evicted)
    }

    /// Remove a single cache entry and its file.
    pub fn remove_entry(&self, cache_key: &str) -> anyhow::Result<()> {
        let path = self.entry_path(cache_key);
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(cache_key, error = %e, "Failed to remove cached file");
            }
        }
        self.db.execute(
            "DELETE FROM cache_entries WHERE cache_key = ?1",
            params![cache_key],
        )?;
        debug!(cache_key, "Cache entry removed");
        Ok(())
    }

    /// Get the block map for a block-mode entry.
    pub fn get_block_map(&self, cache_key: &str) -> anyhow::Result<Option<BlockMap>> {
        let entry = self.get_entry(cache_key)?;
        match entry {
            Some(e) => match e.block_map {
                Some(bytes) => Ok(Some(BlockMap::from_bytes(&bytes))),
                None => Ok(None), // Full-file mode
            },
            None => Ok(None),
        }
    }

    /// Check if a block-mode entry should be promoted to full-file download
    /// (fragmentation guard: too many intervals).
    pub fn should_promote_to_full(&self, cache_key: &str) -> bool {
        match self.get_block_map(cache_key) {
            Ok(Some(bm)) => bm.interval_count() > FRAGMENTATION_LIMIT,
            _ => false,
        }
    }

    /// List all entries ordered by last_access ascending (for eviction).
    pub fn list_lru(&self) -> anyhow::Result<Vec<CacheEntry>> {
        let mut stmt = self.db.prepare(
            "SELECT cache_key, file_id, mtime, size_on_record, block_size, block_map,
                    cached_bytes, last_access, source
             FROM cache_entries ORDER BY last_access ASC",
        )?;
        let entries = stmt.query_map([], |row| {
            Ok(CacheEntry {
                cache_key: row.get(0)?,
                file_id: row.get(1)?,
                mtime: row.get(2)?,
                size_on_record: row.get(3)?,
                block_size: row.get(4)?,
                block_map: row.get(5)?,
                cached_bytes: row.get(6)?,
                last_access: row.get(7)?,
                source: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }
}

/// Get available disk space for a path. Returns u64::MAX on error.
fn available_disk_space(path: &Path) -> u64 {
    // Use statvfs on unix
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok();
        if let Some(c_path) = c_path {
            unsafe {
                let mut stat: libc::statvfs = std::mem::zeroed();
                if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                    return stat.f_bavail as u64 * stat.f_frsize as u64;
                }
            }
        }
        u64::MAX
    }
    #[cfg(not(unix))]
    {
        u64::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache() -> (tempfile::TempDir, FileCache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::open(dir.path()).unwrap();
        (dir, cache)
    }

    #[test]
    fn test_create_cache() {
        let (_dir, cache) = temp_cache();
        assert_eq!(cache.total_cached_bytes().unwrap(), 0);
    }

    #[test]
    fn test_store_and_get_full_file() {
        let (_dir, cache) = temp_cache();
        cache
            .store_full_file("abcdef12", "file::abcdef12", "2025-01-01T00:00:00Z", 1024, "local")
            .unwrap();

        let entry = cache.get_entry("abcdef12").unwrap().unwrap();
        assert_eq!(entry.file_id, "file::abcdef12");
        assert_eq!(entry.size_on_record, 1024);
        assert_eq!(entry.cached_bytes, 1024);
        assert!(entry.block_map.is_none()); // full-file mode
        assert_eq!(entry.source, "local");
    }

    #[test]
    fn test_store_and_get_block_entry() {
        let (_dir, cache) = temp_cache();
        let mut bm = BlockMap::new();
        bm.insert(0..1000);
        bm.insert(5000..6000);

        cache
            .store_block_entry(
                "abcdef12",
                "file::abcdef12",
                "2025-01-01T00:00:00Z",
                100_000_000,
                &bm,
                "remote:node-2",
            )
            .unwrap();

        let entry = cache.get_entry("abcdef12").unwrap().unwrap();
        assert_eq!(entry.cached_bytes, 2000);
        assert!(entry.block_map.is_some());

        let loaded_bm = cache.get_block_map("abcdef12").unwrap().unwrap();
        assert_eq!(loaded_bm.cached_bytes(), 2000);
        assert!(loaded_bm.contains(500));
        assert!(loaded_bm.contains(5500));
        assert!(!loaded_bm.contains(3000));
    }

    #[test]
    fn test_staleness_check() {
        let (_dir, cache) = temp_cache();
        cache
            .store_full_file("abc", "file::abc", "2025-01-01T00:00:00Z", 1024, "local")
            .unwrap();

        // Same mtime and size = not stale
        assert!(!cache.is_stale("abc", "2025-01-01T00:00:00Z", 1024));
        // Different mtime = stale
        assert!(cache.is_stale("abc", "2025-06-01T00:00:00Z", 1024));
        // Different size = stale
        assert!(cache.is_stale("abc", "2025-01-01T00:00:00Z", 2048));
        // Missing entry = stale
        assert!(cache.is_stale("nonexistent", "2025-01-01T00:00:00Z", 1024));
    }

    #[test]
    fn test_total_cached_bytes() {
        let (_dir, cache) = temp_cache();
        cache
            .store_full_file("a1", "file::a1", "2025-01-01T00:00:00Z", 1000, "local")
            .unwrap();
        cache
            .store_full_file("b2", "file::b2", "2025-01-01T00:00:00Z", 2000, "local")
            .unwrap();
        assert_eq!(cache.total_cached_bytes().unwrap(), 3000);
    }

    #[test]
    fn test_remove_entry() {
        let (_dir, cache) = temp_cache();
        cache
            .store_full_file("abc", "file::abc", "2025-01-01T00:00:00Z", 1024, "local")
            .unwrap();
        cache.remove_entry("abc").unwrap();
        assert!(cache.get_entry("abc").unwrap().is_none());
    }

    #[test]
    fn test_evict_lru() {
        let (_dir, cache) = temp_cache();
        // Store entries with different sizes
        for i in 0..10 {
            let key = format!("{:02}abcdef", i);
            cache
                .store_full_file(
                    &key,
                    &format!("file::{}", key),
                    "2025-01-01T00:00:00Z",
                    1000,
                    "local",
                )
                .unwrap();
            // Stagger last_access by touching
            std::thread::sleep(std::time::Duration::from_millis(10));
            cache.touch(&key).unwrap();
        }

        assert_eq!(cache.total_cached_bytes().unwrap(), 10000);
        // Evict until under 5000
        let evicted = cache.evict_lru(5000, 0).unwrap();
        assert!(evicted >= 5);
        assert!(cache.total_cached_bytes().unwrap() <= 5000);
    }

    #[test]
    fn test_should_use_block_mode() {
        assert!(!FileCache::should_use_block_mode(1024));
        assert!(!FileCache::should_use_block_mode(50 * 1024 * 1024));
        assert!(FileCache::should_use_block_mode(50 * 1024 * 1024 + 1));
    }

    #[test]
    fn test_list_lru_order() {
        let (_dir, cache) = temp_cache();
        cache
            .store_full_file("zz", "file::zz", "2025-01-01T00:00:00Z", 100, "local")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        cache
            .store_full_file("aa", "file::aa", "2025-01-01T00:00:00Z", 200, "local")
            .unwrap();

        let entries = cache.list_lru().unwrap();
        assert_eq!(entries.len(), 2);
        // First entry should be the oldest (zz was stored first)
        assert_eq!(entries[0].cache_key, "zz");
    }

    #[test]
    fn test_entry_path_sharding() {
        let (_dir, cache) = temp_cache();
        let path = cache.entry_path("abcdef1234");
        assert!(path.to_string_lossy().contains("/ab/abcdef1234"));
    }

    #[test]
    fn test_fragmentation_guard() {
        let (_dir, cache) = temp_cache();
        let mut bm = BlockMap::new();
        for i in 0..1001 {
            bm.insert((i * 100)..(i * 100 + 50));
        }
        cache
            .store_block_entry("frag", "file::frag", "2025-01-01T00:00:00Z", 200_000, &bm, "remote:n1")
            .unwrap();
        assert!(cache.should_promote_to_full("frag"));
    }
}
