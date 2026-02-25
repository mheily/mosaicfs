mod access_cache;
mod auth;
mod couchdb;
mod credentials;
mod handlers;
mod label_cache;
mod notifications;
mod readdir;
mod readdir_cache;
mod routes;
mod state;
mod tls;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use state::AppState;

const DEFAULT_PORT: u16 = 8443;
const DEFAULT_DATA_DIR: &str = "/var/lib/mosaicfs/server";
const DB_NAME: &str = "mosaicfs";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Bootstrap subcommand: create first credential if none exist
    if std::env::args().nth(1).as_deref() == Some("bootstrap") {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("warn"))
            .init();
        let db = couchdb::CouchClient::from_env(DB_NAME);
        db.ensure_db().await?;
        couchdb::create_indexes(&db).await?;
        let existing = credentials::list_credentials(&db).await?;
        if !existing.is_empty() {
            eprintln!("Credentials already exist. Use the Settings page to create more.");
            std::process::exit(1);
        }
        let (access_key, secret_key) = credentials::create_credential(&db, "admin").await?;
        println!("Access Key: {}", access_key);
        println!("Secret Key: {}", secret_key);
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    info!("mosaicfs-server starting");

    let data_dir = PathBuf::from(
        std::env::var("MOSAICFS_DATA_DIR").unwrap_or_else(|_| DEFAULT_DATA_DIR.to_string()),
    );
    std::fs::create_dir_all(&data_dir)?;

    let port: u16 = std::env::var("MOSAICFS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    // Initialize CouchDB
    let db = couchdb::CouchClient::from_env(DB_NAME);
    db.ensure_db().await?;
    info!("CouchDB connection established");

    // Initialize CouchDB indexes
    couchdb::create_indexes(&db).await?;
    info!("CouchDB indexes verified");

    // Generate or load TLS certificates
    let rustls_config = tls::ensure_tls_certs(&data_dir)?;
    info!("TLS certificates ready");

    // Generate or load JWT signing secret
    let jwt_secret = auth::jwt::ensure_jwt_secret(&data_dir)?;
    info!("JWT signing secret ready");

    // Build materialized caches
    let label_cache = Arc::new(label_cache::LabelCache::new());
    let access_cache = Arc::new(access_cache::AccessCache::new());

    label_cache.build(&db).await?;
    access_cache.build(&db).await?;
    info!("Materialized caches built");

    // Build app state
    let state = Arc::new(AppState::new(db, jwt_secret, Arc::clone(&label_cache), Arc::clone(&access_cache)));

    // Ensure root directory exists
    handlers::vfs::ensure_root_directory(&state).await?;
    info!("Root directory verified");

    // Start access tracking flush task (every 5 minutes)
    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                flush_access_records(&state).await;
            }
        });
        info!("Access tracking flush task started");
    }

    // Start changes feed watcher
    {
        let state = Arc::clone(&state);
        let label_cache = Arc::clone(&label_cache);
        let access_cache = Arc::clone(&access_cache);
        tokio::spawn(async move {
            changes_feed_watcher(&state, &label_cache, &access_cache).await;
        });
        info!("Changes feed watcher started");
    }

    // Start control plane health check task (every 5 minutes)
    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            control_plane_health_checks(&state).await;
        });
        info!("Control plane health check task started");
    }

    // Build router
    let app = routes::build_router(state).layer(TraceLayer::new_for_http());

    // Serve with TLS
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(port = port, "Listening on https://0.0.0.0:{}", port);

    let tls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(rustls_config));
    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}

/// Poll CouchDB _changes feed and dispatch to label/access caches.
async fn changes_feed_watcher(
    state: &AppState,
    label_cache: &label_cache::LabelCache,
    access_cache: &access_cache::AccessCache,
) {
    let mut since = "now".to_string();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    loop {
        interval.tick().await;
        match state.db.changes(&since).await {
            Ok(resp) => {
                for change in &resp.results {
                    if let Some(doc) = &change.doc {
                        let id = change.id.as_str();
                        if id.starts_with("label_assignment::") || id.starts_with("label_rule::") {
                            label_cache.handle_change(doc, change.deleted, &state.db).await;
                        }
                        if id.starts_with("access::") {
                            access_cache.handle_change(doc, change.deleted);
                        }
                        if id.starts_with("dir::") {
                            // Invalidate readdir cache for changed directory
                            if let Some(vpath) = doc.get("virtual_path").and_then(|v| v.as_str()) {
                                state.readdir_cache.invalidate(vpath);
                            }
                        }
                        // Detect persistent CouchDB conflicts
                        if doc.get("_conflicts").and_then(|v| v.as_array()).map_or(false, |a| !a.is_empty()) {
                            let db = state.db.clone();
                            let conflict_id = id.to_string();
                            tokio::spawn(async move {
                                notifications::emit_control_plane_notification(
                                    &db, "couchdb", "persistent_couchdb_conflicts",
                                    "warning", "CouchDB document conflicts detected",
                                    &format!("Document '{}' has unresolved conflicts.", conflict_id),
                                ).await;
                            });
                        }
                    }
                }
                since = resp.last_seq;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Changes feed poll failed");
            }
        }
    }
}

/// Flush pending access records to CouchDB as `access` documents via _bulk_docs
async fn flush_access_records(state: &AppState) {
    let pending = {
        let mut tracker = match state.access_tracker.lock() {
            Ok(t) => t,
            Err(_) => return,
        };
        tracker.take_pending()
    };

    if pending.is_empty() {
        return;
    }

    // Update access cache immediately
    for (file_id, timestamp) in &pending {
        state.access_cache.record_access(file_id, *timestamp);
    }

    let docs: Vec<serde_json::Value> = pending
        .into_iter()
        .map(|(file_id, timestamp)| {
            let file_uuid = file_id.strip_prefix("file::").unwrap_or(&file_id);
            serde_json::json!({
                "_id": format!("access::{}", file_uuid),
                "type": "access",
                "file_id": file_id,
                "last_access": timestamp.to_rfc3339(),
            })
        })
        .collect();

    match state.db.bulk_docs(&docs).await {
        Ok(results) => {
            let ok_count = results.iter().filter(|r| r.ok == Some(true)).count();
            let err_count = results.len() - ok_count;
            if err_count > 0 {
                tracing::warn!(ok = ok_count, errors = err_count, "Access flush partial failure");
            } else {
                tracing::debug!(count = ok_count, "Access records flushed");
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to flush access records");
        }
    }
}

/// Periodic control plane health checks.
async fn control_plane_health_checks(state: &AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));

    loop {
        interval.tick().await;

        // Check control plane disk usage
        if let Ok(output) = tokio::process::Command::new("df")
            .arg("--output=pcent")
            .arg("/")
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().nth(1) {
                if let Some(pct) = line.trim().strip_suffix('%').and_then(|s| s.trim().parse::<u32>().ok()) {
                    if pct >= 90 {
                        notifications::emit_control_plane_notification(
                            &state.db, "system", "control_plane_disk_low",
                            "warning", "Control plane disk low",
                            &format!("Root filesystem usage at {}%.", pct),
                        ).await;
                    } else {
                        notifications::resolve_control_plane_notification(
                            &state.db, "control_plane_disk_low",
                        ).await;
                    }
                }
            }
        }

        // Check for inactive credentials (not used in 90 days)
        if let Ok(creds) = crate::credentials::list_credentials(&state.db).await {
            for cred in &creds {
                let key_id = cred.get("access_key_id").and_then(|v| v.as_str()).unwrap_or("");
                let enabled = cred.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                if !enabled || key_id.is_empty() {
                    continue;
                }
                let last_used = cred.get("last_used").and_then(|v| v.as_str()).unwrap_or("");
                if !last_used.is_empty() {
                    if let Ok(ts) = last_used.parse::<chrono::DateTime<chrono::Utc>>() {
                        let age = chrono::Utc::now() - ts;
                        if age.num_days() > 90 {
                            notifications::emit_control_plane_notification(
                                &state.db, "auth",
                                &format!("credential_inactive:{}", key_id),
                                "warning", "Credential inactive",
                                &format!("Credential '{}' has not been used in {} days.", key_id, age.num_days()),
                            ).await;
                        }
                    }
                }
            }
        }
    }
}
