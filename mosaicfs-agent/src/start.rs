//! Agent startup logic.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mosaicfs_common::config::MosaicfsConfig;
use mosaicfs_common::couchdb::CouchClient;
use mosaicfs_common::notifications;
use mosaicfs_common::secrets::{self, SecretsBackend};
use tokio::signal;
use tokio::time;
use tracing::{error, info};
use uuid::Uuid;

use crate::crawler;
use crate::node;
use crate::replication_subsystem;
use crate::WatchPathProvider;

const DEFAULT_STATE_DIR: &str = "/var/lib/mosaicfs";
const DB_NAME: &str = "mosaicfs";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const CRAWL_INTERVAL: Duration = Duration::from_secs(15);
const REPLICATION_FLUSH_INTERVAL_S: u64 = 60;
const REPLICATION_FULL_SCAN_INTERVAL_S: u64 = 86400; // daily

/// Start the agent subsystem. Runs until a shutdown signal is received.
///
/// Expects `features.agent = true` and a populated `[agent]` section.
pub async fn start_agent(
    cfg: Arc<MosaicfsConfig>,
    secrets: Arc<dyn SecretsBackend>,
    provider: Arc<dyn WatchPathProvider>,
) -> anyhow::Result<()> {
    let agent_cfg = cfg
        .agent
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("[agent] section missing"))?;

    info!("mosaicfs agent starting");

    let state_dir = agent_cfg
        .state_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR));

    let node_id = resolve_node_id(cfg.node.node_id.as_deref(), &state_dir)?;
    info!(node_id = %node_id, "Agent identity resolved");

    let couchdb_url = secrets.get(secrets::names::COUCHDB_URL)?;
    let couchdb_user = secrets.get(secrets::names::COUCHDB_USER)?;
    let couchdb_password = secrets.get(secrets::names::COUCHDB_PASSWORD)?;

    let db = CouchClient::new(&couchdb_url, DB_NAME, &couchdb_user, &couchdb_password);
    db.ensure_db().await?;
    info!("CouchDB connection established");

    node::register_node(&db, &node_id, &agent_cfg.watch_paths).await?;

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

    info!("Starting initial filesystem crawl");
    let opened = provider.open().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "watch path provider failed for initial crawl");
        vec![]
    });
    let crawl_paths: Vec<PathBuf> = opened.iter().map(|o| o.path.clone()).collect();
    let result = crawler::crawl(
        &db,
        &node_id,
        &crawl_paths,
        &agent_cfg.excluded_paths,
        replication_handle.as_ref(),
    )
    .await?;
    drop(opened);
    info!(
        new = result.files_new,
        updated = result.files_updated,
        skipped = result.files_skipped,
        deleted = result.files_deleted,
        "Initial crawl complete"
    );

    notifications::emit_notification(
        &db,
        &node_id,
        "crawler",
        "first_crawl_complete",
        "info",
        "Initial crawl complete",
        &format!(
            "Indexed {} new, {} updated, {} deleted files.",
            result.files_new, result.files_updated, result.files_deleted,
        ),
        None,
    )
    .await;

    info!("Agent running. Press Ctrl+C to stop.");
    let mut heartbeat_interval = time::interval(HEARTBEAT_INTERVAL);
    let mut health_check_interval = time::interval(Duration::from_secs(300));
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
                let opened = provider.open().unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "watch path provider failed for periodic crawl");
                    vec![]
                });
                let crawl_paths: Vec<PathBuf> = opened.iter().map(|o| o.path.clone()).collect();
                if let Err(e) = crawler::crawl(
                    &db,
                    &node_id,
                    &crawl_paths,
                    &agent_cfg.excluded_paths,
                    replication_handle.as_ref(),
                ).await {
                    error!(error = %e, "Periodic crawl failed");
                }
                drop(opened);
            }
            _ = health_check_interval.tick() => {
                check_inotify_limits(&db, &node_id).await;
                check_storage_capacity(&db, &node_id, &agent_cfg.watch_paths).await;
            }
            _ = shutdown_signal() => {
                info!("Received shutdown signal");
                break;
            }
        }
    }

    node::set_offline(&db, &node_id).await?;
    info!("Agent stopped");
    Ok(())
}

/// Resolve node_id: prefer the config field, fall back to persisted state,
/// otherwise generate and persist a new one.
fn resolve_node_id(from_config: Option<&str>, state_dir: &Path) -> anyhow::Result<String> {
    if let Some(id) = from_config {
        return Ok(id.to_string());
    }
    let node_id_file = state_dir.join("node_id");
    if node_id_file.exists() {
        return Ok(std::fs::read_to_string(&node_id_file)?.trim().to_string());
    }
    let id = format!("node-{}", &Uuid::new_v4().to_string()[..8]);
    std::fs::create_dir_all(state_dir)?;
    std::fs::write(&node_id_file, &id)?;
    tracing::info!(node_id = %id, "Generated new node_id");
    Ok(id)
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.ok() };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to register SIGTERM handler")
            .recv()
            .await
    };
    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c  => {},
        _ = sigterm => {},
    }
}

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
    let _ = (db, node_id);
}

async fn check_storage_capacity(db: &CouchClient, node_id: &str, watch_paths: &[PathBuf]) {
    for watch_path in watch_paths {
        if let Ok(output) = tokio::process::Command::new("df")
            .arg("--output=pcent")
            .arg(watch_path)
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().nth(1) {
                if let Some(pct) = line
                    .trim()
                    .strip_suffix('%')
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
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
    notifications::resolve_notification(db, node_id, "storage_near_capacity").await;
}
