//! `mosaicfs-server` legacy binary — thin wrapper around
//! `mosaicfs_server::start_web_ui`.
//!
//! Kept for phase 2 of change 006 so the two-binary build still works.
//! Phase 3 replaces it with the unified `mosaicfs` binary.

use std::path::PathBuf;
use std::sync::Arc;

use mosaicfs_common::config::MosaicfsConfig;
use mosaicfs_server::{run_bootstrap, start_web_ui};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_PATH: &str = "/etc/mosaicfs/mosaicfs.toml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args: Vec<String> = std::env::args().collect();

    // Subcommand: bootstrap
    if args.get(1).map(|s| s.as_str()) == Some("bootstrap") {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("warn"))
            .init();
        let cfg = load_config(&args)?;
        let json_output = args.iter().any(|a| a == "--json");
        return run_bootstrap(&cfg, json_output).await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let cfg = Arc::new(load_config(&args)?);
    start_web_ui(cfg).await
}

fn load_config(args: &[String]) -> anyhow::Result<MosaicfsConfig> {
    let path = arg_value(args, "--config").unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    MosaicfsConfig::load(&PathBuf::from(path))
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
