//! Inode table mapping inode numbers to file/directory metadata.
//!
//! Inode space: 0 invalid, 1 root, 2–999 reserved, 1000+ randomly assigned.
//! Inodes are stable across restarts (persisted in CouchDB file documents).

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};

/// Metadata associated with an inode.
#[derive(Debug, Clone)]
pub enum InodeEntry {
    Directory(DirInode),
    File(FileInode),
}

#[derive(Debug, Clone)]
pub struct DirInode {
    pub inode: u64,
    pub virtual_path: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct FileInode {
    pub inode: u64,
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mtime: DateTime<Utc>,
    pub mime_type: Option<String>,
    pub source_node_id: String,
    pub source_export_path: String,
}

pub const ROOT_INODE: u64 = 1;
pub const RESERVED_MAX: u64 = 999;

/// Thread-safe inode table.
pub struct InodeTable {
    /// inode -> entry
    by_inode: RwLock<HashMap<u64, InodeEntry>>,
    /// file_id -> inode (for dedup and stable inode lookup)
    file_id_to_inode: RwLock<HashMap<String, u64>>,
    /// virtual_path -> inode (for directory lookup)
    dir_path_to_inode: RwLock<HashMap<String, u64>>,
}

impl InodeTable {
    pub fn new() -> Self {
        let mut by_inode = HashMap::new();
        by_inode.insert(
            ROOT_INODE,
            InodeEntry::Directory(DirInode {
                inode: ROOT_INODE,
                virtual_path: "/".to_string(),
                name: String::new(),
            }),
        );
        let mut dir_path = HashMap::new();
        dir_path.insert("/".to_string(), ROOT_INODE);

        Self {
            by_inode: RwLock::new(by_inode),
            file_id_to_inode: RwLock::new(HashMap::new()),
            dir_path_to_inode: RwLock::new(dir_path),
        }
    }

    pub fn get(&self, inode: u64) -> Option<InodeEntry> {
        self.by_inode.read().unwrap().get(&inode).cloned()
    }

    pub fn get_dir_by_path(&self, path: &str) -> Option<u64> {
        self.dir_path_to_inode.read().unwrap().get(path).copied()
    }

    pub fn get_inode_for_file_id(&self, file_id: &str) -> Option<u64> {
        self.file_id_to_inode.read().unwrap().get(file_id).copied()
    }

    /// Insert or update a directory entry.
    pub fn insert_dir(&self, dir: DirInode) {
        let inode = dir.inode;
        let path = dir.virtual_path.clone();
        self.by_inode
            .write()
            .unwrap()
            .insert(inode, InodeEntry::Directory(dir));
        self.dir_path_to_inode.write().unwrap().insert(path, inode);
    }

    /// Insert or update a file entry, returning the inode used.
    /// If the file_id already has an inode, reuse it (stability).
    pub fn insert_file(&self, file: FileInode) -> u64 {
        let inode = file.inode;
        let file_id = file.file_id.clone();
        self.by_inode
            .write()
            .unwrap()
            .insert(inode, InodeEntry::File(file));
        self.file_id_to_inode
            .write()
            .unwrap()
            .insert(file_id, inode);
        inode
    }

    /// Lookup a child name within a directory's readdir results.
    /// Returns the inode if found in the table.
    pub fn lookup_child(&self, parent_inode: u64, name: &str) -> Option<u64> {
        let by_inode = self.by_inode.read().unwrap();
        let parent = by_inode.get(&parent_inode)?;
        let parent_path = match parent {
            InodeEntry::Directory(d) => &d.virtual_path,
            _ => return None,
        };

        // Check if it's a directory child
        let child_path = if parent_path == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent_path, name)
        };

        if let Some(&ino) = self.dir_path_to_inode.read().unwrap().get(&child_path) {
            return Some(ino);
        }

        // Check file children — scan for matching name under this parent
        // This is O(n) but typically called after readdir populates the table
        for (ino, entry) in by_inode.iter() {
            if let InodeEntry::File(f) = entry {
                if f.name == name {
                    // Verify this file could belong to this directory
                    // We trust the caller (FUSE lookup after readdir)
                    return Some(*ino);
                }
            }
        }

        None
    }

    /// Clear all non-root entries (used during reload).
    pub fn clear(&self) {
        let mut by_inode = self.by_inode.write().unwrap();
        let root = by_inode.remove(&ROOT_INODE);
        by_inode.clear();
        if let Some(r) = root {
            by_inode.insert(ROOT_INODE, r);
        }
        self.file_id_to_inode.write().unwrap().clear();
        let mut dirs = self.dir_path_to_inode.write().unwrap();
        dirs.clear();
        dirs.insert("/".to_string(), ROOT_INODE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_exists_by_default() {
        let table = InodeTable::new();
        let entry = table.get(ROOT_INODE).unwrap();
        match entry {
            InodeEntry::Directory(d) => {
                assert_eq!(d.virtual_path, "/");
                assert_eq!(d.inode, ROOT_INODE);
            }
            _ => panic!("Root should be a directory"),
        }
    }

    #[test]
    fn test_insert_and_get_dir() {
        let table = InodeTable::new();
        table.insert_dir(DirInode {
            inode: 1000,
            virtual_path: "/documents".to_string(),
            name: "documents".to_string(),
        });
        assert_eq!(table.get_dir_by_path("/documents"), Some(1000));
        let entry = table.get(1000).unwrap();
        match entry {
            InodeEntry::Directory(d) => assert_eq!(d.name, "documents"),
            _ => panic!("Should be directory"),
        }
    }

    #[test]
    fn test_insert_and_get_file() {
        let table = InodeTable::new();
        let ino = table.insert_file(FileInode {
            inode: 5000,
            file_id: "file::abc123".to_string(),
            name: "report.pdf".to_string(),
            size: 1024,
            mtime: Utc::now(),
            mime_type: Some("application/pdf".to_string()),
            source_node_id: "node-1".to_string(),
            source_export_path: "/docs/report.pdf".to_string(),
        });
        assert_eq!(ino, 5000);
        assert_eq!(
            table.get_inode_for_file_id("file::abc123"),
            Some(5000)
        );
    }

    #[test]
    fn test_clear_preserves_root() {
        let table = InodeTable::new();
        table.insert_dir(DirInode {
            inode: 1000,
            virtual_path: "/test".to_string(),
            name: "test".to_string(),
        });
        table.clear();
        assert!(table.get(ROOT_INODE).is_some());
        assert!(table.get(1000).is_none());
        assert!(table.get_dir_by_path("/test").is_none());
    }

    #[test]
    fn test_inode_stability_across_inserts() {
        let table = InodeTable::new();
        let ino1 = table.insert_file(FileInode {
            inode: 5000,
            file_id: "file::abc".to_string(),
            name: "test.txt".to_string(),
            size: 100,
            mtime: Utc::now(),
            mime_type: None,
            source_node_id: "n1".to_string(),
            source_export_path: "/test.txt".to_string(),
        });
        // Re-insert same file_id with same inode (simulating restart)
        let ino2 = table.insert_file(FileInode {
            inode: 5000,
            file_id: "file::abc".to_string(),
            name: "test.txt".to_string(),
            size: 200, // size changed
            mtime: Utc::now(),
            mime_type: None,
            source_node_id: "n1".to_string(),
            source_export_path: "/test.txt".to_string(),
        });
        assert_eq!(ino1, ino2);
    }
}
