//! Unified MosaicFS binary.
//!
//! Loads `/etc/mosaicfs/mosaicfs.toml` (or the `--config` path), inspects
//! `[features]`, and starts only the subsystems enabled there. Each node
//! decides its role via config — a NAS enables `agent + web_ui`, a
//! laptop enables `agent + vfs`, a headless indexer enables `agent`
//! alone.

use std::path::PathBuf;
use std::sync::Arc;

use mosaicfs_agent::start_agent;
use mosaicfs_common::config::MosaicfsConfig;
use mosaicfs_server::{run_bootstrap, start_web_ui};
use mosaicfs_vfs::start_vfs;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_PATH: &str = "/etc/mosaicfs/mosaicfs.toml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args: Vec<String> = std::env::args().collect();

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
    info!(
        agent = cfg.features.agent,
        vfs = cfg.features.vfs,
        web_ui = cfg.features.web_ui,
        "mosaicfs starting"
    );

    let mut set: tokio::task::JoinSet<(&'static str, anyhow::Result<()>)> = tokio::task::JoinSet::new();

    if cfg.features.web_ui {
        let cfg = Arc::clone(&cfg);
        set.spawn(async move { ("web_ui", start_web_ui(cfg).await) });
    }
    if cfg.features.agent {
        let cfg = Arc::clone(&cfg);
        set.spawn(async move { ("agent", start_agent(cfg).await) });
    }
    if cfg.features.vfs {
        let cfg = Arc::clone(&cfg);
        set.spawn(async move { ("vfs", start_vfs(cfg).await) });
    }

    if set.is_empty() {
        anyhow::bail!("no features enabled — set at least one of agent/vfs/web_ui");
    }

    // Wait until the first subsystem exits. If any subsystem dies, exit
    // the whole binary so the supervisor (podman, systemd, launchd)
    // restarts the pod rather than leaving half of it running silently.
    if let Some(join) = set.join_next().await {
        match join {
            Ok((name, Ok(()))) => info!(subsystem = name, "subsystem exited cleanly"),
            Ok((name, Err(e))) => error!(subsystem = name, error = %e, "subsystem returned error"),
            Err(e) => error!(error = %e, "subsystem task panicked"),
        }
    }
    set.shutdown().await;
    Ok(())
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
