use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mosaicfs_agent::{OpenedWatchPath, WatchPathProvider};

use crate::bookmarks::BookmarkStore;
use crate::commands::ResolveBookmarkError;

pub struct BookmarkedWatchPathProvider {
    store: Arc<Mutex<BookmarkStore>>,
    app_data_dir: PathBuf,
}

impl BookmarkedWatchPathProvider {
    pub fn new(store: Arc<Mutex<BookmarkStore>>, app_data_dir: PathBuf) -> Self {
        Self { store, app_data_dir }
    }
}

impl WatchPathProvider for BookmarkedWatchPathProvider {
    fn open(&self) -> anyhow::Result<Vec<OpenedWatchPath>> {
        let settings = crate::settings::load(&self.app_data_dir);
        let mut result = Vec::new();

        for path_str in &settings.watch_paths {
            let bookmark_data = {
                let store = self.store.lock().unwrap();
                store.get(path_str).map(|b| b.to_vec())
            };

            let Some(data) = bookmark_data else {
                tracing::warn!(path = %path_str, "watch path has no bookmark, skipping");
                continue;
            };

            match crate::macos::resolve_bookmark(&data) {
                Ok(guard) => {
                    let resolved = guard.path().to_path_buf();
                    if resolved.display().to_string() != *path_str {
                        tracing::info!(
                            settings_path = %path_str,
                            resolved_path = %resolved.display(),
                            "watch path resolved to different location (directory moved)"
                        );
                    }
                    result.push(OpenedWatchPath {
                        path: resolved,
                        _guard: Box::new(guard),
                    });
                }
                Err(ResolveBookmarkError::Stale) => {
                    tracing::warn!(path = %path_str, "watch path bookmark is stale, removing");
                    let _ = self.store.lock().unwrap().remove(path_str);
                }
                Err(ResolveBookmarkError::Other(msg)) => {
                    tracing::warn!(path = %path_str, error = %msg, "watch path bookmark failed to resolve, skipping");
                }
            }
        }

        Ok(result)
    }
}
