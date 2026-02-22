//! FUSE filesystem implementation using `fuser`.
//!
//! Implements lookup, getattr, readdir, open, and read.
//! Uses a tokio runtime handle to bridge async CouchDB queries
//! into fuser's synchronous callback interface.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyOpen,
    Request,
};
use tracing::warn;

use crate::cache::FileCache;
use crate::couchdb::CouchClient;
use crate::inode::{DirInode, FileInode, InodeEntry, InodeTable};
use crate::readdir::{self, ReaddirDirEntry, VfsStepContext};
use crate::tiered_access::{self, AccessResult, NetworkMountInfo};

const TTL: Duration = Duration::from_secs(5);
const BLOCK_SIZE: u32 = 512;

/// Configuration for the MosaicFS FUSE mount.
pub struct FuseConfig {
    pub node_id: String,
    pub watch_paths: Vec<PathBuf>,
    pub network_mounts: Vec<NetworkMountInfo>,
    pub mount_point: PathBuf,
    pub cache_dir: PathBuf,
    /// Max cache size in bytes (default 10 GB).
    pub cache_cap: u64,
    /// Minimum free disk space in bytes (default 1 GB).
    pub min_free_space: u64,
}

impl Default for FuseConfig {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            watch_paths: Vec::new(),
            network_mounts: Vec::new(),
            mount_point: PathBuf::from("/mnt/mosaicfs"),
            cache_dir: PathBuf::from("/var/lib/mosaicfs/cache"),
            cache_cap: 10 * 1024 * 1024 * 1024,
            min_free_space: 1024 * 1024 * 1024,
        }
    }
}

/// The MosaicFS FUSE filesystem.
pub struct MosaicFs {
    db: CouchClient,
    config: FuseConfig,
    inodes: Arc<InodeTable>,
    cache: Arc<FileCache>,
    rt: tokio::runtime::Handle,
    /// Tracks readdir results per directory inode for subsequent lookup calls.
    /// Maps (parent_inode, child_name) -> child_inode.
    dir_children: RwLock<HashMap<(u64, String), u64>>,
    /// Open file handles: fh -> (file_inode, PathBuf to read from)
    open_files: Mutex<HashMap<u64, (FileInode, PathBuf)>>,
    next_fh: Mutex<u64>,
    /// Access tracking: file_id -> last recorded time (debounced)
    access_tracker: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl MosaicFs {
    pub fn new(
        db: CouchClient,
        config: FuseConfig,
        rt: tokio::runtime::Handle,
    ) -> anyhow::Result<Self> {
        let cache = FileCache::open(&config.cache_dir)?;

        Ok(Self {
            db,
            config,
            inodes: Arc::new(InodeTable::new()),
            cache: Arc::new(cache),
            rt,
            dir_children: RwLock::new(HashMap::new()),
            open_files: Mutex::new(HashMap::new()),
            next_fh: Mutex::new(1),
            access_tracker: Mutex::new(HashMap::new()),
        })
    }

    /// Load all virtual directories into the inode table at startup.
    pub fn load_directories(&self) -> anyhow::Result<()> {
        let dirs = self.rt.block_on(async {
            self.db.all_docs_by_prefix("dir::", true).await
        })?;

        for row in dirs.rows {
            if let Some(doc) = row.doc {
                let inode = doc.get("inode").and_then(|v| v.as_u64()).unwrap_or(0);
                let vpath = doc
                    .get("virtual_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = doc
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if inode > 0 && !vpath.is_empty() {
                    self.inodes.insert_dir(DirInode {
                        inode,
                        virtual_path: vpath,
                        name,
                    });
                }
            }
        }

        Ok(())
    }

    fn make_dir_attr(&self, inode: u64) -> FileAttr {
        let now = SystemTime::now();
        FileAttr {
            ino: inode,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: BLOCK_SIZE,
            flags: 0,
        }
    }

    fn make_file_attr(&self, file: &FileInode) -> FileAttr {
        let mtime = chrono_to_system_time(file.mtime);
        FileAttr {
            ino: file.inode,
            size: file.size,
            blocks: (file.size + 511) / 512,
            atime: mtime,
            mtime,
            ctime: mtime,
            crtime: mtime,
            kind: FileType::RegularFile,
            perm: 0o444, // read-only
            nlink: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: BLOCK_SIZE,
            flags: 0,
        }
    }

    fn alloc_fh(&self) -> u64 {
        let mut fh = self.next_fh.lock().unwrap();
        let val = *fh;
        *fh += 1;
        val
    }

    fn record_access(&self, file_id: &str) {
        let now = Utc::now();
        let mut tracker = self.access_tracker.lock().unwrap();
        // Debounce: only track if >1 hour since last recorded
        if let Some(last) = tracker.get(file_id) {
            if (now - *last).num_hours() < 1 {
                return;
            }
        }
        tracker.insert(file_id.to_string(), now);
    }
}

impl Filesystem for MosaicFs {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Check dir_children cache first (populated by readdir)
        if let Some(&child_ino) = self
            .dir_children
            .read()
            .unwrap()
            .get(&(parent, name_str.to_string()))
        {
            if let Some(entry) = self.inodes.get(child_ino) {
                match entry {
                    InodeEntry::Directory(_) => {
                        reply.entry(&TTL, &self.make_dir_attr(child_ino), 0);
                        return;
                    }
                    InodeEntry::File(f) => {
                        reply.entry(&TTL, &self.make_file_attr(&f), 0);
                        return;
                    }
                }
            }
        }

        // Fallback: check inode table directly
        if let Some(ino) = self.inodes.lookup_child(parent, name_str) {
            if let Some(entry) = self.inodes.get(ino) {
                match entry {
                    InodeEntry::Directory(_) => {
                        reply.entry(&TTL, &self.make_dir_attr(ino), 0);
                        return;
                    }
                    InodeEntry::File(f) => {
                        reply.entry(&TTL, &self.make_file_attr(&f), 0);
                        return;
                    }
                }
            }
        }

        reply.error(libc::ENOENT);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        match self.inodes.get(ino) {
            Some(InodeEntry::Directory(_)) => {
                reply.attr(&TTL, &self.make_dir_attr(ino));
            }
            Some(InodeEntry::File(f)) => {
                reply.attr(&TTL, &self.make_file_attr(&f));
            }
            None => {
                reply.error(libc::ENOENT);
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let dir_entry = match self.inodes.get(ino) {
            Some(InodeEntry::Directory(d)) => d,
            _ => {
                reply.error(libc::ENOTDIR);
                return;
            }
        };

        let virtual_path = dir_entry.virtual_path.clone();

        // Fetch directory document and evaluate readdir
        let result = self.rt.block_on(async {
            let doc_id = readdir::dir_id_for(&virtual_path);
            let doc = self.db.get_document(&doc_id).await.ok();

            let mounts: Vec<mosaicfs_common::documents::MountEntry> = doc
                .as_ref()
                .and_then(|d| d.get("mounts"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let inherited_steps =
                readdir::collect_inherited_steps(&self.db, &virtual_path).await.unwrap_or_default();

            // Get child directories
            let child_dirs_result = self.db.all_docs_by_prefix("dir::", true).await;
            let child_dir_entries: Vec<ReaddirDirEntry> = child_dirs_result
                .map(|resp| {
                    resp.rows
                        .into_iter()
                        .filter_map(|row| {
                            let doc = row.doc?;
                            let pp = doc.get("parent_path")?.as_str()?;
                            if pp != virtual_path {
                                return None;
                            }
                            let name = doc.get("name")?.as_str()?.to_string();
                            let dir_inode = doc.get("inode")?.as_u64()?;
                            let vp = doc.get("virtual_path")?.as_str()?.to_string();
                            Some(ReaddirDirEntry {
                                name,
                                inode: dir_inode,
                                virtual_path: vp,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            let child_dir_names: Vec<String> =
                child_dir_entries.iter().map(|d| d.name.clone()).collect();

            let ctx = VfsStepContext::empty();
            let files = readdir::evaluate_readdir(
                &self.db,
                &mounts,
                &inherited_steps,
                &child_dir_names,
                &ctx,
            )
            .await
            .unwrap_or_default();

            (files, child_dir_entries)
        });

        let (files, child_dirs) = result;

        // Build full entry list: ".", "..", child dirs, files
        let mut entries: Vec<(u64, FileType, String)> = Vec::new();
        entries.push((ino, FileType::Directory, ".".to_string()));
        entries.push((ino, FileType::Directory, "..".to_string()));

        // Register child directories in inode table and dir_children map
        for dir in &child_dirs {
            self.inodes.insert_dir(DirInode {
                inode: dir.inode,
                virtual_path: dir.virtual_path.clone(),
                name: dir.name.clone(),
            });
            self.dir_children
                .write()
                .unwrap()
                .insert((ino, dir.name.clone()), dir.inode);
            entries.push((dir.inode, FileType::Directory, dir.name.clone()));
        }

        // Register files in inode table and dir_children map
        for file in &files {
            let file_inode = FileInode {
                inode: file.inode,
                file_id: file.file_id.clone(),
                name: file.name.clone(),
                size: file.size,
                mtime: file.mtime,
                mime_type: file.mime_type.clone(),
                source_node_id: file.source_node_id.clone(),
                source_export_path: file.source_export_path.clone(),
            };
            self.inodes.insert_file(file_inode);
            self.dir_children
                .write()
                .unwrap()
                .insert((ino, file.name.clone()), file.inode);
            entries.push((file.inode, FileType::RegularFile, file.name.clone()));
        }

        // Reply with entries starting from offset
        for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            let full = reply.add(*entry_ino, (i + 1) as i64, *kind, name.as_str());
            if full {
                break;
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        // Read-only filesystem — reject writes
        let accmode = flags & libc::O_ACCMODE;
        if accmode != libc::O_RDONLY {
            reply.error(libc::EROFS);
            return;
        }

        let file = match self.inodes.get(ino) {
            Some(InodeEntry::File(f)) => f,
            _ => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Record access
        self.record_access(&file.file_id);

        // Resolve file through tiered access
        let access_result = self.rt.block_on(tiered_access::resolve_file(
            &file,
            &self.config.node_id,
            &self.config.watch_paths,
            &self.config.network_mounts,
            &self.cache,
            &self.db,
        ));

        match access_result {
            AccessResult::LocalPath(path) => {
                let fh = self.alloc_fh();
                self.open_files
                    .lock()
                    .unwrap()
                    .insert(fh, (file.clone(), path));
                reply.opened(fh, 0);
            }
            AccessResult::NeedsFetch(fetch_info) => {
                // Attempt remote fetch
                let fetch_result = self.rt.block_on(fetch_remote_file(
                    &fetch_info,
                    &self.cache,
                    &self.db,
                    &self.config,
                ));

                match fetch_result {
                    Ok(path) => {
                        let fh = self.alloc_fh();
                        self.open_files
                            .lock()
                            .unwrap()
                            .insert(fh, (file.clone(), path));
                        reply.opened(fh, 0);
                    }
                    Err(e) => {
                        warn!(file_id = %file.file_id, error = %e, "Remote fetch failed");
                        reply.error(libc::EIO);
                    }
                }
            }
            AccessResult::NotAccessible(msg) => {
                warn!(file_id = %file.file_id, reason = %msg, "File not accessible");
                reply.error(libc::ENOENT);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let (_, path) = match self.open_files.lock().unwrap().get(&fh) {
            Some(entry) => entry.clone(),
            None => {
                reply.error(libc::EBADF);
                return;
            }
        };

        // Read from the resolved path
        match std::fs::File::open(&path) {
            Ok(mut file) => {
                use std::io::{Read, Seek, SeekFrom};
                if let Err(e) = file.seek(SeekFrom::Start(offset as u64)) {
                    warn!(error = %e, "Seek failed");
                    reply.error(libc::EIO);
                    return;
                }
                let mut buf = vec![0u8; size as usize];
                match file.read(&mut buf) {
                    Ok(n) => {
                        reply.data(&buf[..n]);
                    }
                    Err(e) => {
                        warn!(error = %e, "Read failed");
                        reply.error(libc::EIO);
                    }
                }
            }
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to open cached/local file");
                reply.error(libc::EIO);
            }
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        self.open_files.lock().unwrap().remove(&fh);
        reply.ok();
    }
}

/// Fetch a file from a remote agent and cache it.
async fn fetch_remote_file(
    info: &tiered_access::FetchInfo,
    cache: &FileCache,
    _db: &CouchClient,
    config: &FuseConfig,
) -> anyhow::Result<PathBuf> {
    let file_uuid = info
        .file_id
        .strip_prefix("file::")
        .unwrap_or(&info.file_id);

    let url = format!(
        "{}/api/agent/transfer/{}",
        info.transfer_endpoint.trim_end_matches('/'),
        urlencoding::encode(&info.file_id)
    );

    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Remote fetch failed: HTTP {}",
            resp.status()
        );
    }

    // Get digest header if present (for verification)
    let digest_header = resp
        .headers()
        .get("Digest")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let staging_path = cache.staging_path();
    let bytes = resp.bytes().await?;

    // Verify digest if present
    if let Some(digest_str) = digest_header {
        if let Some(expected) = parse_digest_header(&digest_str) {
            use sha2::Digest;
            let actual = sha2::Sha256::digest(&bytes);
            if actual.as_slice() != expected.as_slice() {
                anyhow::bail!("Digest verification failed for {}", info.file_id);
            }
        }
    }

    // Write to staging
    std::fs::write(&staging_path, &bytes)?;

    // Atomic move to final location
    let final_path = cache.entry_path(file_uuid);
    let shard_dir = final_path.parent().unwrap();
    std::fs::create_dir_all(shard_dir)?;
    std::fs::rename(&staging_path, &final_path)?;

    // Update cache index
    let source = format!("remote:{}", info.node_id);
    if FileCache::should_use_block_mode(info.size) {
        let mut bm = crate::block_map::BlockMap::new();
        bm.insert(0..bytes.len() as u64);
        cache.store_block_entry(file_uuid, &info.file_id, &info.mtime, info.size, &bm, &source)?;
    } else {
        cache.store_full_file(file_uuid, &info.file_id, &info.mtime, info.size, &source)?;
    }

    // Run eviction check
    let _ = cache.evict_lru(config.cache_cap, config.min_free_space);

    Ok(final_path)
}

fn parse_digest_header(header: &str) -> Option<Vec<u8>> {
    // Format: "sha-256=:base64data:"
    let prefix = "sha-256=:";
    let value = header.strip_prefix(prefix)?;
    let value = value.strip_suffix(':')?;
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(value).ok()
}

fn chrono_to_system_time(dt: DateTime<Utc>) -> SystemTime {
    let secs = dt.timestamp();
    let nanos = dt.timestamp_subsec_nanos();
    if secs >= 0 {
        UNIX_EPOCH + Duration::new(secs as u64, nanos)
    } else {
        UNIX_EPOCH
    }
}

/// Mount the MosaicFS FUSE filesystem (blocking).
pub fn mount(
    db: CouchClient,
    config: FuseConfig,
    rt: tokio::runtime::Handle,
) -> anyhow::Result<()> {
    let mount_point = config.mount_point.clone();
    std::fs::create_dir_all(&mount_point)?;

    let fs = MosaicFs::new(db, config, rt)?;
    fs.load_directories()?;

    let options = vec![
        fuser::MountOption::RO,
        fuser::MountOption::FSName("mosaicfs".to_string()),
        fuser::MountOption::AutoUnmount,
        fuser::MountOption::AllowOther,
    ];

    fuser::mount2(fs, &mount_point, &options)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn test_chrono_to_system_time() {
        let dt = chrono::DateTime::parse_from_rfc3339("2025-01-15T10:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let st = chrono_to_system_time(dt);
        let duration = st.duration_since(UNIX_EPOCH).unwrap();
        assert_eq!(duration.as_secs(), dt.timestamp() as u64);
    }

    #[test]
    fn test_parse_digest_header() {
        // Valid digest
        let hash = sha2::Sha256::digest(b"hello world");
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &hash,
        );
        let header = format!("sha-256=:{}:", encoded);
        let parsed = parse_digest_header(&header).unwrap();
        assert_eq!(parsed, hash.as_slice());

        // Invalid format
        assert!(parse_digest_header("invalid").is_none());
        assert!(parse_digest_header("sha-256=:bad_base64:").is_none());
    }

    #[test]
    fn test_fuse_config_defaults() {
        let config = FuseConfig::default();
        assert_eq!(config.cache_cap, 10 * 1024 * 1024 * 1024);
        assert_eq!(config.min_free_space, 1024 * 1024 * 1024);
    }
}
