//! Web UI startup logic.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use mosaicfs_common::config::MosaicfsConfig;
use mosaicfs_common::couchdb::CouchClient;
use mosaicfs_common::notifications;
use mosaicfs_common::secrets::{self, SecretsBackend};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::access_cache::AccessCache;
use crate::auth;
use crate::credentials;
use crate::handlers;
use crate::label_cache::LabelCache;
use crate::routes;
use crate::state::AppState;
use crate::tls;

const DEFAULT_DATA_DIR: &str = "/var/lib/mosaicfs/server";
const DB_NAME: &str = "mosaicfs";

/// Build the axum [`Router`](axum::Router) for the web UI subsystem and spawn
/// the long-lived background tasks (changes feed, access flush, control-plane
/// health checks). The caller is responsible for installing the rustls crypto
/// provider and for binding the returned router to a transport.
///
/// Used both by [`start_web_ui`] (the binary path) and by the Tauri desktop
/// app, which serves the router in-process to avoid sandbox/codesign issues
/// that would otherwise be incurred by spawning the server as a child process.
pub async fn build_app_router(
    cfg: Arc<MosaicfsConfig>,
    secrets: Arc<dyn SecretsBackend>,
) -> anyhow::Result<axum::Router> {
    let web = cfg
        .web_ui
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("[web_ui] section missing"))?;

    let data_dir = web
        .data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
    std::fs::create_dir_all(&data_dir)?;

    if web.developer_mode {
        tracing::warn!("Developer mode enabled — DELETE /api/system/data is active");
    }

    let couchdb_url = secrets.get(secrets::names::COUCHDB_URL)?;
    let couchdb_user = secrets.get(secrets::names::COUCHDB_USER)?;
    let couchdb_password = secrets.get(secrets::names::COUCHDB_PASSWORD)?;

    let db = CouchClient::new(&couchdb_url, DB_NAME, &couchdb_user, &couchdb_password);
    db.ensure_db().await?;
    info!("CouchDB connection established");

    mosaicfs_common::couchdb::create_indexes(&db).await?;
    info!("CouchDB indexes verified");

    let jwt_secret = auth::jwt::ensure_jwt_secret(&data_dir)?;
    info!("JWT signing secret ready");

    let existing_credentials = credentials::list_credentials(&db).await?;
    if existing_credentials.is_empty() {
        let bootstrap_token = uuid::Uuid::new_v4().to_string();
        let token_path = data_dir.join("bootstrap_token");
        std::fs::write(&token_path, &bootstrap_token)?;
        info!("the bootstrap token is {}", bootstrap_token);
    }

    let label_cache = Arc::new(LabelCache::new());
    let access_cache = Arc::new(AccessCache::new());
    label_cache.build(&db).await?;
    access_cache.build(&db).await?;
    info!("Materialized caches built");

    // Resolve this node's ID so the open-file feature can translate remote
    // paths via network mounts. Prefer the explicit config value; fall back
    // to the persisted file written by the agent subsystem.
    let node_id = cfg.node.node_id.clone().or_else(|| {
        cfg.agent
            .as_ref()
            .and_then(|a| a.state_dir.as_ref())
            .map(|d| d.join("node_id"))
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    if let Some(ref id) = node_id {
        info!(node_id = %id, "Server node identity resolved");
    }

    let state = Arc::new(AppState::new(
        db,
        jwt_secret,
        data_dir.clone(),
        couchdb_url.clone(),
        couchdb_user.clone(),
        couchdb_password.clone(),
        Arc::clone(&label_cache),
        Arc::clone(&access_cache),
        web.developer_mode,
        node_id,
    ));

    handlers::vfs::ensure_root_directory(&state).await?;
    info!("Root directory verified");

    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                flush_access_records(&state).await;
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let label_cache = Arc::clone(&label_cache);
        let access_cache = Arc::clone(&access_cache);
        tokio::spawn(async move {
            changes_feed_watcher(&state, &label_cache, &access_cache).await;
        });
    }
    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            control_plane_health_checks(&state).await;
        });
    }
    info!("Background tasks started");

    Ok(routes::build_router(state).layer(TraceLayer::new_for_http()))
}

/// Start the web UI subsystem. Runs until the HTTP server shuts down.
///
/// Expects `features.web_ui = true` and a populated `[web_ui]` section.
/// The caller is responsible for installing the rustls crypto provider
/// exactly once before calling this function.
pub async fn start_web_ui(
    cfg: Arc<MosaicfsConfig>,
    secrets: Arc<dyn SecretsBackend>,
) -> anyhow::Result<()> {
    let web = cfg
        .web_ui
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("[web_ui] section missing"))?
        .clone();

    let data_dir = web
        .data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));

    let addr: SocketAddr = web
        .listen
        .parse()
        .map_err(|e| anyhow::anyhow!("[web_ui].listen invalid: {e}"))?;

    let insecure_http = web.insecure_http;

    // Admin session layer + middleware read this env var; propagate the
    // config-resolved value so they stay in sync whether the binary was
    // invoked with env overrides or a TOML-only config.
    if insecure_http {
        unsafe { std::env::set_var("MOSAICFS_INSECURE_HTTP", "1"); }
    }

    info!("mosaicfs web_ui starting");

    let rustls_config = if insecure_http {
        tracing::warn!("web_ui.insecure_http is set — serving plain HTTP (dev only)");
        None
    } else {
        let cfg = tls::ensure_tls_certs(&data_dir)?;
        info!("TLS certificates ready");
        Some(cfg)
    };

    // Unix socket mode: skip TLS, bind directly to the socket path.
    // Auth is unconditionally disabled — the socket's filesystem permissions
    // are the security boundary, not credentials.
    #[cfg(unix)]
    if let Some(ref socket_path) = web.socket_path {
        unsafe { std::env::set_var("MOSAICFS_INSECURE_HTTP", "1"); }
        let app = build_app_router(cfg, secrets).await?;
        let _ = std::fs::remove_file(socket_path);
        let listener = tokio::net::UnixListener::bind(socket_path)?;
        info!(socket = %socket_path.display(), "Listening on unix socket");
        axum::serve(listener, app.into_make_service()).await?;
        return Ok(());
    }

    let app = build_app_router(cfg, secrets).await?;

    // TCP mode: insecure HTTP binds to loopback only; TLS honours the configured address.
    let bind_addr = if insecure_http {
        SocketAddr::from(([127, 0, 0, 1], addr.port()))
    } else {
        addr
    };
    let scheme = if rustls_config.is_some() { "https" } else { "http" };
    info!(%bind_addr, "Listening on {}://{}", scheme, bind_addr);

    match rustls_config {
        Some(cfg) => {
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(cfg));
            axum_server::bind_rustls(bind_addr, tls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await?;
        }
        None => {
            axum_server::bind(bind_addr)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await?;
        }
    }

    Ok(())
}

/// Run the `bootstrap` subcommand: create the first credential if none exist.
/// Prints access/secret keys to stdout.
pub async fn run_bootstrap(
    _cfg: &MosaicfsConfig,
    secrets: Arc<dyn SecretsBackend>,
    json_output: bool,
) -> anyhow::Result<()> {
    let couchdb_url = secrets.get(secrets::names::COUCHDB_URL)?;
    let couchdb_user = secrets.get(secrets::names::COUCHDB_USER)?;
    let couchdb_password = secrets.get(secrets::names::COUCHDB_PASSWORD)?;

    let db = CouchClient::new(&couchdb_url, DB_NAME, &couchdb_user, &couchdb_password);
    db.ensure_db().await?;
    mosaicfs_common::couchdb::create_indexes(&db).await?;
    let existing = credentials::list_credentials(&db).await?;
    if !existing.is_empty() {
        eprintln!("Credentials already exist. Use the Settings page to create more.");
        std::process::exit(1);
    }
    let (access_key, secret_key) = credentials::create_credential(&db, "admin").await?;
    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "access_key_id": access_key,
                "secret_key": secret_key,
            })
        );
    } else {
        println!("Access Key: {}", access_key);
        println!("Secret Key: {}", secret_key);
    }
    Ok(())
}

/// Poll CouchDB _changes feed and dispatch to label/access caches.
async fn changes_feed_watcher(
    state: &AppState,
    label_cache: &LabelCache,
    access_cache: &AccessCache,
) {
    let mut since = "now".to_string();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    loop {
        interval.tick().await;
        match state.db.changes(&since, true, Some(1000)).await {
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
                            if let Some(vpath) = doc.get("virtual_path").and_then(|v| v.as_str()) {
                                state.readdir_cache.invalidate(vpath);
                            }
                        }
                        if doc
                            .get("_conflicts")
                            .and_then(|v| v.as_array())
                            .map_or(false, |a| !a.is_empty())
                        {
                            let db = state.db.clone();
                            let conflict_id = id.to_string();
                            tokio::spawn(async move {
                                notifications::emit_control_plane_notification(
                                    &db,
                                    "couchdb",
                                    "persistent_couchdb_conflicts",
                                    "warning",
                                    "CouchDB document conflicts detected",
                                    &format!(
                                        "Document '{}' has unresolved conflicts.",
                                        conflict_id
                                    ),
                                )
                                .await;
                            });
                        }
                    }
                }
                since = resp.last_seq_string();
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

        if let Ok(output) = tokio::process::Command::new("df")
            .arg("--output=pcent")
            .arg("/")
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
                        notifications::emit_control_plane_notification(
                            &state.db,
                            "system",
                            "control_plane_disk_low",
                            "warning",
                            "Control plane disk low",
                            &format!("Root filesystem usage at {}%.", pct),
                        )
                        .await;
                    } else {
                        notifications::resolve_control_plane_notification(
                            &state.db,
                            "control_plane_disk_low",
                        )
                        .await;
                    }
                }
            }
        }

        if let Ok(creds) = credentials::list_credentials(&state.db).await {
            for cred in &creds {
                let key_id = cred
                    .get("access_key_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let enabled = cred.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                if !enabled || key_id.is_empty() {
                    continue;
                }
                let last_used = cred
                    .get("last_used")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !last_used.is_empty() {
                    if let Ok(ts) = last_used.parse::<chrono::DateTime<chrono::Utc>>() {
                        let age = chrono::Utc::now() - ts;
                        if age.num_days() > 90 {
                            notifications::emit_control_plane_notification(
                                &state.db,
                                "auth",
                                &format!("credential_inactive:{}", key_id),
                                "warning",
                                "Credential inactive",
                                &format!(
                                    "Credential '{}' has not been used in {} days.",
                                    key_id,
                                    age.num_days()
                                ),
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
}
