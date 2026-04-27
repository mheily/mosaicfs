use std::any::Any;
use std::path::PathBuf;

pub trait WatchPathProvider: Send + Sync {
    fn open(&self) -> anyhow::Result<Vec<OpenedWatchPath>>;
}

pub struct OpenedWatchPath {
    pub path: PathBuf,
    pub _guard: Box<dyn Any + Send + Sync>,
}

/// Bare provider: returns configured paths with no-op guards.
/// Used on Linux and in the containerized server.
pub struct BareWatchPathProvider {
    paths: Vec<PathBuf>,
}

impl BareWatchPathProvider {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }
}

impl WatchPathProvider for BareWatchPathProvider {
    fn open(&self) -> anyhow::Result<Vec<OpenedWatchPath>> {
        Ok(self
            .paths
            .iter()
            .map(|p| OpenedWatchPath {
                path: p.clone(),
                _guard: Box::new(()),
            })
            .collect())
    }
}
