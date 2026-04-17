//! `mosaicfs-agent` legacy binary — thin wrapper around
//! `mosaicfs_agent::start_agent`.
//!
//! Kept for phase 2 of change 006 so the two-binary build still works.
//! Phase 3 replaces it with the unified `mosaicfs` binary.

use std::path::PathBuf;
use std::sync::Arc;

use mosaicfs_agent::start_agent;
use mosaicfs_common::config::MosaicfsConfig;
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_PATH: &str = "/etc/mosaicfs/mosaicfs.toml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let path = arg_value(&args, "--config")
        .or_else(|| args.get(1).filter(|s| !s.starts_with("--")).cloned())
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());

    let cfg = Arc::new(MosaicfsConfig::load(&PathBuf::from(path))?);
    start_agent(cfg).await
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(rest) = a.strip_prefix(&format!("{}=", name)) {
            return Some(rest.to_string());
        }
    }
    None
}
