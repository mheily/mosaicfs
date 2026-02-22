mod access_cache;
mod auth;
mod couchdb;
mod credentials;
mod handlers;
mod label_cache;
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
