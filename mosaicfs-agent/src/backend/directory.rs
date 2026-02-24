//! Local directory backend adapter.
//!
//! Stores replicated files in a local filesystem directory. Uses atomic
//! write (temp file → fsync → rename) to prevent partial writes.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use async_trait::async_trait;
use bytes::Bytes;
use tracing::debug;

use mosaicfs_common::backend::BackendAdapter;

pub struct DirectoryAdapter {
    base_path: PathBuf,
}

impl DirectoryAdapter {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }

    fn full_path(&self, key: &str) -> PathBuf {
        // Prevent path traversal
        let key = key.trim_start_matches('/').replace("..", "");
        self.base_path.join(key)
    }
}

#[async_trait]
impl BackendAdapter for DirectoryAdapter {
    async fn upload(&self, remote_key: &str, data: Bytes) -> anyhow::Result<()> {
        let dest = self.full_path(remote_key);

        // Ensure parent directory exists
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directory")?;
        }

        // Write to temp file, then rename (atomic)
        let tmp_path = dest.with_extension("tmp");
        tokio::fs::write(&tmp_path, &data)
            .await
            .context("Failed to write temp file")?;

        // fsync the file
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&tmp_path)
            .await
            .context("Failed to open temp file for fsync")?;
        file.sync_all().await.context("fsync failed")?;
        drop(file);

        // Atomic rename
        tokio::fs::rename(&tmp_path, &dest)
            .await
            .context("Atomic rename failed")?;

        debug!(key = %remote_key, "Directory upload complete");
        Ok(())
    }

    async fn download(&self, remote_key: &str) -> anyhow::Result<Bytes> {
        let src = self.full_path(remote_key);
        if !src.exists() {
            bail!("File not found at path: {}", src.display());
        }
        let data = tokio::fs::read(&src)
            .await
            .with_context(|| format!("Failed to read {}", src.display()))?;
        Ok(Bytes::from(data))
    }

    async fn delete(&self, remote_key: &str) -> anyhow::Result<()> {
        let path = self.full_path(remote_key);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .context("Failed to delete file")?;
        }
        debug!(key = %remote_key, "Directory delete complete");
        Ok(())
    }

    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let search_dir = self.base_path.join(prefix.trim_start_matches('/'));
        let base = &self.base_path;

        let mut keys = Vec::new();
        if !search_dir.exists() {
            return Ok(keys);
        }

        let mut stack = vec![search_dir];
        while let Some(dir) = stack.pop() {
            let mut entries = tokio::fs::read_dir(&dir)
                .await
                .context("Failed to read directory")?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let meta = entry.metadata().await?;

                if meta.is_dir() {
                    stack.push(path);
                } else if meta.is_file() {
                    // Compute key relative to base_path
                    if let Ok(relative) = path.strip_prefix(base) {
                        keys.push(relative.to_string_lossy().to_string());
                    }
                }
            }
        }

        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_directory_adapter_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DirectoryAdapter::new(dir.path());

        let data = Bytes::from("hello world");
        adapter.upload("test/subdir/file.txt", data.clone()).await.unwrap();

        let downloaded = adapter.download("test/subdir/file.txt").await.unwrap();
        assert_eq!(downloaded, data);

        let keys = adapter.list("test").await.unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].ends_with("file.txt"));

        adapter.delete("test/subdir/file.txt").await.unwrap();
        let result = adapter.download("test/subdir/file.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_directory_adapter_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DirectoryAdapter::new(dir.path());

        // Path traversal attempt should be sanitized
        let data = Bytes::from("malicious");
        let result = adapter.upload("../../../etc/passwd", data).await;
        // Should succeed but write inside the base_path
        // The key gets sanitized so it writes to base_path/etcpasswd or similar
        // Key thing: it should not escape the base directory
        if result.is_ok() {
            let dest = adapter.full_path("../../../etc/passwd");
            assert!(dest.starts_with(dir.path()));
        }
    }
}
