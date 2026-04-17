//! Entry point for the `vfs` feature.
//!
//! Stub: the FUSE mount plumbing (`fuse_fs::mount`) exists but has not
//! yet been wired into an end-to-end feature path. This function logs
//! that the feature is enabled and returns without mounting. A later
//! change will build the filesystem view from CouchDB state and hand it
//! to `fuser::mount2` on a blocking thread.

use std::sync::Arc;

use mosaicfs_common::config::MosaicfsConfig;
use tracing::warn;

pub async fn start_vfs(cfg: Arc<MosaicfsConfig>) -> anyhow::Result<()> {
    let vfs = cfg
        .vfs
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("[vfs] section missing"))?;
    warn!(
        mount_point = %vfs.mount_point.display(),
        "vfs feature enabled but the FUSE mount is not yet implemented — continuing without a mount",
    );
    // Park forever so the task does not exit; the host binary waits on it
    // alongside the other subsystems.
    std::future::pending::<()>().await;
    Ok(())
}
