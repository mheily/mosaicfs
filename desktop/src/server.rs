//! In-process axum server.
//!
//! The desktop app embeds the mosaicfs web_ui directly: a TCP listener bound
//! to `127.0.0.1:<random>` accepts connections from the WKWebView, and each
//! connection is served by an axum [`Router`] held in memory. There is no
//! child process and no Unix socket — both of those introduced macOS sandbox
//! and code-signing problems that have no upside in a single-user GUI app.
//!
//! The router lives behind an [`ArcSwapOption`] so that re-configuring the
//! CouchDB connection (via the setup form) just builds a new router and
//! atomically swaps it in. While no router is loaded — at first launch
//! before settings exist, or while the new router is being built — every
//! request is answered with a tiny "Connecting…" HTML page that retries
//! every 2 s. WKWebView blanks on non-2xx responses so we always return 200.

use std::sync::Arc;

use arc_swap::ArcSwapOption;
use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use mosaicfs_common::config::{
    CouchdbConfig, FeaturesConfig, MosaicfsConfig, NodeConfig, SecretsConfig, WebUiFeatureConfig,
};
use mosaicfs_common::couchdb::CouchClient;
use mosaicfs_common::secrets::InlineBackend;
use tokio::io::AsyncWriteExt;

use crate::settings::Settings;

/// Tauri managed state: the localhost port the in-process server listens on.
pub struct ProxyPort(pub u16);

/// Tauri managed state: the currently-active router. `None` while no settings
/// are configured (or while a rebuild is in flight after a save).
#[derive(Default)]
pub struct RouterSlot(pub ArcSwapOption<Router>);

impl RouterSlot {
    pub fn new() -> Self {
        Self(ArcSwapOption::const_empty())
    }

    pub fn set(&self, router: Router) {
        self.0.store(Some(Arc::new(router)));
    }
}

/// Bind a localhost TCP listener and start the accept loop. Returns the port.
/// Each accepted connection snapshots the current [`Router`] from `slot` and
/// serves it with hyper http/1.1; if no router is loaded the connection gets
/// the retry HTML page instead.
pub fn start(slot: Arc<RouterSlot>) -> std::io::Result<u16> {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = std_listener.local_addr()?.port();
    std_listener.set_nonblocking(true)?;

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mosaicfs-desktop: listener error: {e}");
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let slot = Arc::clone(&slot);
            tokio::spawn(async move {
                serve_connection(stream, slot).await;
            });
        }
    });

    Ok(port)
}

async fn serve_connection(mut stream: tokio::net::TcpStream, slot: Arc<RouterSlot>) {
    if let Some(router) = slot.0.load_full() {
        let svc = TowerToHyperService::new((*router).clone());
        let _ = http1::Builder::new()
            .serve_connection(TokioIo::new(stream), svc)
            .await;
    } else {
        // No router loaded yet — serve the connecting page.
        let body = CONNECTING_HTML;
        let resp = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    }
}

const CONNECTING_HTML: &str = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>MosaicFS — Connecting…</title>
<style>
  body{font-family:-apple-system,system-ui,sans-serif;text-align:center;
       padding:60px 40px;color:#555;}
</style></head><body>
<h2>MosaicFS</h2>
<p>Connecting to server…</p>
<p style="font-size:.9em;color:#999;">Open <strong>Connection…</strong> from the menu bar
to configure the CouchDB connection.</p>
<script>setTimeout(function(){location.reload();},2000);</script>
</body></html>"#;

/// Build a [`MosaicfsConfig`] from the desktop's settings, suitable for
/// constructing an in-process router. `data_dir` is where the server keeps
/// its TLS certs / JWT secret / bootstrap token (none of which we end up
/// using for TLS, but the server still wants the directory).
pub fn config_from_settings(settings: &Settings, app_data_dir: &std::path::Path) -> MosaicfsConfig {
    let data_dir = app_data_dir.join("server-data");
    MosaicfsConfig {
        node: NodeConfig::default(),
        features: FeaturesConfig {
            agent: false,
            vfs: false,
            web_ui: true,
        },
        couchdb: CouchdbConfig {
            url: settings.couchdb_url.clone(),
            user: settings.couchdb_user.clone(),
            password: settings.couchdb_password.clone(),
        },
        agent: None,
        vfs: None,
        web_ui: Some(WebUiFeatureConfig {
            listen: "127.0.0.1:0".into(),
            data_dir: Some(data_dir),
            insecure_http: true,
            developer_mode: false,
            socket_path: None,
        }),
        credentials: None,
        secrets: SecretsConfig {
            manager: "inline".into(),
        },
    }
}

/// Build the in-process router for the given settings. Sets the auth-bypass
/// env var first (the loopback TCP listener is the trust boundary, the same
/// way the unix-socket mode treated socket fs perms).
///
/// Returns the router and the resolved node_id so the caller can share the
/// same identity with the in-process agent.
pub async fn build_router(
    settings: &Settings,
    app_data_dir: &std::path::Path,
) -> anyhow::Result<(Router, Option<String>)> {
    unsafe { std::env::set_var("MOSAICFS_INSECURE_HTTP", "1"); }
    let node_id = resolve_node_id(settings).await;
    let mut cfg = config_from_settings(settings, app_data_dir);
    cfg.node.node_id = node_id.clone();
    let cfg = Arc::new(cfg);
    let secrets = Arc::new(InlineBackend::from_config(&cfg, None));
    let router = mosaicfs_server::build_app_router(cfg, secrets).await?;
    Ok((router, node_id))
}

/// Resolve the desktop's node identity.
///
/// Priority:
///   1. `MOSAICFS_NODE_ID` env var — skips CouchDB lookup entirely.
///   2. CouchDB lookup by `machine_id` field on `node::*` docs.
///
/// Returns `None` when no match is found (open-file will degrade gracefully).
pub async fn resolve_node_id(settings: &Settings) -> Option<String> {
    if let Ok(id) = std::env::var("MOSAICFS_NODE_ID") {
        if !id.is_empty() {
            tracing::info!(node_id = %id, "Node identity from MOSAICFS_NODE_ID");
            return Some(id);
        }
    }

    let machine_id = mosaicfs_common::machine_id::get();
    let db = CouchClient::new(
        &settings.couchdb_url,
        "mosaicfs",
        &settings.couchdb_user,
        &settings.couchdb_password,
    );

    match find_node_by_machine_id(&db, &machine_id).await {
        Ok(Some(node_id)) => {
            tracing::info!(node_id = %node_id, machine_id = %machine_id, "Node identity resolved via machine_id");
            Some(node_id)
        }
        Ok(None) => {
            tracing::warn!(machine_id = %machine_id, "No node doc found for this machine; open-file will be unavailable");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to look up node by machine_id");
            None
        }
    }
}

/// Query CouchDB for the node doc whose `machine_id` field matches.
/// Returns `None` when zero or multiple docs match.
async fn find_node_by_machine_id(
    db: &CouchClient,
    machine_id: &str,
) -> anyhow::Result<Option<String>> {
    let resp = db
        .find(serde_json::json!({
            "type": "node",
            "machine_id": machine_id
        }))
        .await?;

    match resp.docs.len() {
        0 => Ok(None),
        1 => {
            let raw = resp.docs[0]
                .get("_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Strip "node::" prefix — cfg.node.node_id stores the bare ID.
            Ok(Some(raw.strip_prefix("node::").unwrap_or(raw).to_string()))
        }
        n => {
            tracing::warn!(machine_id = %machine_id, count = n,
                "Multiple node docs match this machine_id; skipping auto-select");
            Ok(None)
        }
    }
}

