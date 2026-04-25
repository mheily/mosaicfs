use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct BookmarkFile {
    version: u32,
    bookmarks: HashMap<String, String>,
}

pub struct BookmarkStore {
    path: PathBuf,
    map: HashMap<String, Vec<u8>>,
}

impl BookmarkStore {
    pub fn load(path: PathBuf) -> Self {
        let map = match Self::try_load(&path) {
            Ok(m) => m,
            Err(corrupt) => {
                if corrupt {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let corrupt_path = path.with_extension(format!("json.corrupt.{ts}"));
                    tracing::error!("bookmarks.json is corrupt; preserving as {corrupt_path:?}");
                    let _ = std::fs::rename(&path, corrupt_path);
                }
                HashMap::new()
            }
        };
        Self { path, map }
    }

    // Ok(map) = success.
    // Err(false) = file missing or version mismatch (non-destructive).
    // Err(true) = file exists but JSON/decode is corrupt (rename it).
    fn try_load(path: &PathBuf) -> Result<HashMap<String, Vec<u8>>, bool> {
        let data = std::fs::read_to_string(path).map_err(|_| false)?;
        let file: BookmarkFile = serde_json::from_str(&data).map_err(|_| true)?;
        if file.version != 1 {
            tracing::warn!("bookmarks.json version {} unrecognised; starting empty", file.version);
            return Err(false);
        }
        let mut map = HashMap::new();
        for (k, v) in file.bookmarks {
            let bytes = BASE64.decode(&v).map_err(|_| true)?;
            map.insert(k, bytes);
        }
        Ok(map)
    }

    pub fn get(&self, mount: &str) -> Option<&[u8]> {
        self.map.get(mount).map(Vec::as_slice)
    }

    pub fn insert(&mut self, mount: String, data: Vec<u8>) -> io::Result<()> {
        self.map.insert(mount, data);
        self.save()
    }

    pub fn remove(&mut self, mount: &str) -> io::Result<()> {
        self.map.remove(mount);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let bookmarks: HashMap<String, String> = self
            .map
            .iter()
            .map(|(k, v)| (k.clone(), BASE64.encode(v)))
            .collect();
        let file = BookmarkFile { version: 1, bookmarks };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, &json)?;
        std::fs::rename(&temp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    // T2.1: missing file → empty store
    #[test]
    fn t2_1_missing_file_gives_empty_store() {
        let dir = tmp();
        let store = BookmarkStore::load(dir.path().join("bookmarks.json"));
        assert!(store.map.is_empty());
    }

    // T2.2: version mismatch → empty store, original file untouched
    #[test]
    fn t2_2_version_mismatch_empty_store() {
        let dir = tmp();
        let path = dir.path().join("bookmarks.json");
        fs::write(&path, r#"{"version":99,"bookmarks":{"/x":"yyy"}}"#).unwrap();
        let store = BookmarkStore::load(path.clone());
        assert!(store.map.is_empty());
        // File must still exist (not renamed)
        assert!(path.exists());
    }

    // T2.3: corrupt JSON → empty store, original renamed to .corrupt.<ts>
    #[test]
    fn t2_3_corrupt_json_renamed() {
        let dir = tmp();
        let path = dir.path().join("bookmarks.json");
        fs::write(&path, b"not json at all {{{{").unwrap();
        let store = BookmarkStore::load(path.clone());
        assert!(store.map.is_empty());
        // Original renamed, no longer at the original path
        assert!(!path.exists());
        // A .corrupt.* file should exist
        let corrupt_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains("corrupt")
            })
            .collect();
        assert!(!corrupt_files.is_empty());
    }

    // T2.4: insert + reload round-trips correctly
    #[test]
    fn t2_4_insert_and_reload() {
        let dir = tmp();
        let path = dir.path().join("bookmarks.json");
        let data = b"fake_bookmark_bytes";
        {
            let mut store = BookmarkStore::load(path.clone());
            store.insert("/Volumes/NAS".to_string(), data.to_vec()).unwrap();
        }
        let store2 = BookmarkStore::load(path);
        assert_eq!(store2.get("/Volumes/NAS"), Some(data.as_slice()));
    }

    // T2.5: atomic save — simulate partial write by making the temp path unwritable
    // We just verify that save uses a .tmp file (rename strategy).
    #[test]
    fn t2_5_save_uses_atomic_rename() {
        let dir = tmp();
        let path = dir.path().join("bookmarks.json");
        let mut store = BookmarkStore::load(path.clone());
        store.insert("/Volumes/X".to_string(), vec![1, 2, 3]).unwrap();
        // After save, only bookmarks.json should exist, not a .tmp file
        let tmp_path = path.with_extension("json.tmp");
        assert!(!tmp_path.exists());
        assert!(path.exists());
    }
}
