mod config;
mod couchdb;
mod crawler;
mod node;
mod replication;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    info!("mosaicfs-agent starting");

    // Load config
    let config_path = std::env::args()
        .nth(1)
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

    // Initial crawl
    info!("Starting initial filesystem crawl");
    let result = crawler::crawl(&db, &node_id, &config.watch_paths, &config.excluded_paths).await?;
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
