mod backend;
mod config;
mod couchdb;
mod crawler;
mod init;
mod node;
mod replication;
mod replication_subsystem;

use std::path::PathBuf;
use std::time::Duration;

use tokio::signal;
use tokio::time;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use config::AgentConfig;
use couchdb::CouchClient;

const DEFAULT_CONFIG_PATH: &str = "agent.toml";
const DEFAULT_STATE_DIR: &str = "/var/lib/mosaicfs";
const DB_NAME: &str = "mosaicfs";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const REPLICATION_FLUSH_INTERVAL_S: u64 = 60;
const REPLICATION_FULL_SCAN_INTERVAL_S: u64 = 86400; // daily

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    info!("mosaicfs-agent starting");

    // Check for init subcommand
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("init") {
        return init::run_init().await;
    }

    // Load config
    let config_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    let mut config = AgentConfig::load(&PathBuf::from(&config_path))?;

    let state_dir = PathBuf::from(
        std::env::var("MOSAICFS_STATE_DIR").unwrap_or_else(|_| DEFAULT_STATE_DIR.to_string()),
    );
    let node_id = config.resolve_node_id(&state_dir)?;
    info!(node_id = %node_id, "Agent identity resolved");

    // Connect to CouchDB
    let db = CouchClient::from_env(DB_NAME);
    db.ensure_db().await?;
    info!("CouchDB connection established");

    // Register node
    node::register_node(&db, &node_id, &config.watch_paths).await?;

    // Start replication subsystem
    let replication_handle = match replication_subsystem::start(replication_subsystem::ReplicationConfig {
        node_id: node_id.clone(),
        state_dir: state_dir.clone(),
        db: db.clone(),
        flush_interval_s: REPLICATION_FLUSH_INTERVAL_S,
        full_scan_interval_s: REPLICATION_FULL_SCAN_INTERVAL_S,
    }) {
        Ok(h) => {
            info!("Replication subsystem started");
            Some(h)
        }
        Err(e) => {
            error!(error = %e, "Failed to start replication subsystem");
            None
        }
    };

    // Initial crawl
    info!("Starting initial filesystem crawl");
    let result = crawler::crawl(
        &db, &node_id, &config.watch_paths, &config.excluded_paths,
        replication_handle.as_ref(),
    ).await?;
    info!(
        new = result.files_new,
        updated = result.files_updated,
        skipped = result.files_skipped,
        deleted = result.files_deleted,
        "Initial crawl complete"
    );

    // Start heartbeat loop and wait for shutdown signal
    info!("Agent running. Press Ctrl+C to stop.");
    let mut heartbeat_interval = time::interval(HEARTBEAT_INTERVAL);

    loop {
        tokio::select! {
            _ = heartbeat_interval.tick() => {
                if let Err(e) = node::heartbeat(&db, &node_id).await {
                    error!(error = %e, "Heartbeat failed");
                }
            }
            _ = signal::ctrl_c() => {
                info!("Received shutdown signal");
                break;
            }
        }
    }

    // Clean shutdown
    node::set_offline(&db, &node_id).await?;
    info!("Agent stopped");
    Ok(())
}
