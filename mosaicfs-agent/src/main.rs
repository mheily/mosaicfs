mod backend;
mod config;
mod couchdb;
mod crawler;
mod file_server;
mod init;
mod node;
mod notifications;
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
const CRAWL_INTERVAL: Duration = Duration::from_secs(15);
const FILE_SERVER_PORT: u16 = 8444;
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

    // Load config — accept both positional and --config <path>
    let config_path = if args.get(1).map(|s| s.as_str()) == Some("--config") {
        args.get(2)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("--config requires a path argument"))?
    } else {
        args.get(1)
            .cloned()
            .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string())
    };
    let mut config = AgentConfig::load(&PathBuf::from(&config_path))?;

    let state_dir = PathBuf::from(
        std::env::var("MOSAICFS_STATE_DIR").unwrap_or_else(|_| DEFAULT_STATE_DIR.to_string()),
    );
    let node_id = config.resolve_node_id(&state_dir)?;
    info!(node_id = %node_id, "Agent identity resolved");

    // Determine file server URL using container hostname (resolvable by other services)
    let file_server_host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());
    let file_server_url = format!("http://{}:{}", file_server_host, FILE_SERVER_PORT);

    // Generate a per-run agent token for the internal file server
    let agent_token = uuid::Uuid::new_v4().to_string();

    // Start the file server in the background
    {
        let token = agent_token.clone();
        tokio::spawn(async move {
            file_server::start(token, FILE_SERVER_PORT).await;
        });
    }

    // Connect to CouchDB
    let db = CouchClient::from_env(DB_NAME);
    db.ensure_db().await?;
    info!("CouchDB connection established");

    // Register node (includes file_server_url and agent_token)
    node::register_node(&db, &node_id, &config.watch_paths, &file_server_url, &agent_token).await?;

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

    // Emit first_crawl_complete notification
    notifications::emit_notification(
        &db, &node_id, "crawler", "first_crawl_complete",
        "info", "Initial crawl complete",
        &format!(
            "Indexed {} new, {} updated, {} deleted files.",
            result.files_new, result.files_updated, result.files_deleted,
        ),
        None,
    ).await;

    // Start main loop: heartbeat, periodic re-crawl, health checks, shutdown
    info!("Agent running. Press Ctrl+C to stop.");
    let mut heartbeat_interval = time::interval(HEARTBEAT_INTERVAL);
    let mut health_check_interval = time::interval(Duration::from_secs(300)); // 5 min
    // First tick fires immediately; delay the first periodic crawl since we just ran one
    let mut crawl_interval = time::interval_at(
        tokio::time::Instant::now() + CRAWL_INTERVAL,
        CRAWL_INTERVAL,
    );

    loop {
        tokio::select! {
            _ = heartbeat_interval.tick() => {
                if let Err(e) = node::heartbeat(&db, &node_id).await {
                    error!(error = %e, "Heartbeat failed");
                }
            }
            _ = crawl_interval.tick() => {
                if let Err(e) = crawler::crawl(
                    &db, &node_id, &config.watch_paths, &config.excluded_paths,
                    replication_handle.as_ref(),
                ).await {
                    error!(error = %e, "Periodic crawl failed");
                }
            }
            _ = health_check_interval.tick() => {
                check_inotify_limits(&db, &node_id).await;
                check_storage_capacity(&db, &node_id, &config.watch_paths).await;
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

/// Check inotify watch limits on Linux and emit/resolve notification.
async fn check_inotify_limits(db: &CouchClient, node_id: &str) {
    #[cfg(target_os = "linux")]
    {
        let max = tokio::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
            .await
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let current = tokio::fs::read_to_string("/proc/sys/fs/inotify/max_user_instances")
            .await
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());

        if let (Some(max_w), Some(cur)) = (max, current) {
            if max_w > 0 && cur > max_w * 80 / 100 {
                notifications::emit_notification(
                    db, node_id, "system", "inotify_limit_approaching",
                    "warning", "inotify watch limit approaching",
                    &format!("Using ~{} of {} max inotify watches.", cur, max_w),
                    None,
                ).await;
            } else {
                notifications::resolve_notification(db, node_id, "inotify_limit_approaching").await;
            }
        }
    }
}

/// Check disk usage of watch paths and emit/resolve notification.
async fn check_storage_capacity(db: &CouchClient, node_id: &str, watch_paths: &[std::path::PathBuf]) {
    for watch_path in watch_paths {
        if let Ok(output) = tokio::process::Command::new("df")
            .arg("--output=pcent")
            .arg(watch_path)
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse percentage from df output (second line, e.g. " 85%")
            if let Some(line) = stdout.lines().nth(1) {
                if let Some(pct) = line.trim().strip_suffix('%').and_then(|s| s.trim().parse::<u32>().ok()) {
                    if pct >= 90 {
                        notifications::emit_notification(
                            db, node_id, "storage", "storage_near_capacity",
                            "warning", "Storage near capacity",
                            &format!("Disk usage at {}% for {}.", pct, watch_path.display()),
                            None,
                        ).await;
                        return;
                    }
                }
            }
        }
    }
    // All paths OK — resolve if previously raised
    notifications::resolve_notification(db, node_id, "storage_near_capacity").await;
}
