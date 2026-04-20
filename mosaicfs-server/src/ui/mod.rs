//! Server-side rendered admin UI (change 005).
//!
//! Mounted under `/admin`. Uses Tera for templates, tower-sessions for
//! cookie-based auth, and HTMX for in-page interactivity. Assets (Pico CSS,
//! HTMX) are embedded at compile time.

use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Request, State},
    http::{header, StatusCode, Uri},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;
use tera::{Context, Tera};
use tower_sessions::cookie::time::Duration;
use tower_sessions::{cookie::SameSite, MemoryStore, Session, SessionManagerLayer};

use crate::credentials;
use crate::state::AppState;
use mosaicfs_common::couchdb::CouchError;

pub mod actions;
mod views;

const SESSION_USER_KEY: &str = "access_key_id";
pub(crate) const FLASH_KEY: &str = "_flash";
pub(crate) const NEW_SECRET_KEY: &str = "_new_secret";

static TERA: OnceLock<Tera> = OnceLock::new();

fn tera() -> &'static Tera {
    TERA.get_or_init(|| {
        let mut tera = Tera::default();
        tera.add_raw_templates(vec![
            ("layout.html", include_str!("../../templates/layout.html")),
            ("login.html", include_str!("../../templates/login.html")),
            ("status.html", include_str!("../../templates/status.html")),
            (
                "status_panel.html",
                include_str!("../../templates/status_panel.html"),
            ),
            ("nodes.html", include_str!("../../templates/nodes.html")),
            (
                "nodes_panel.html",
                include_str!("../../templates/nodes_panel.html"),
            ),
            (
                "notifications.html",
                include_str!("../../templates/notifications.html"),
            ),
            (
                "notifications_panel.html",
                include_str!("../../templates/notifications_panel.html"),
            ),
            (
                "replication.html",
                include_str!("../../templates/replication.html"),
            ),
            (
                "replication_panel.html",
                include_str!("../../templates/replication_panel.html"),
            ),
            (
                "bootstrap.html",
                include_str!("../../templates/bootstrap.html"),
            ),
            (
                "settings_credentials.html",
                include_str!("../../templates/settings_credentials.html"),
            ),
            (
                "settings_backup.html",
                include_str!("../../templates/settings_backup.html"),
            ),
            (
                "node_detail.html",
                include_str!("../../templates/node_detail.html"),
            ),
            (
                "storage_backends.html",
                include_str!("../../templates/storage_backends.html"),
            ),
            ("browse.html", include_str!("../../templates/browse.html")),
            ("vfs.html", include_str!("../../templates/vfs.html")),
            ("vfs_new.html", include_str!("../../templates/vfs_new.html")),
            (
                "vfs_dir.html",
                include_str!("../../templates/vfs_dir.html"),
            ),
        ])
        .expect("templates compile");
        tera
    })
}

pub(crate) fn render(name: &str, ctx: &Context) -> Response {
    match tera().render(name, ctx) {
        Ok(body) => Html(body).into_response(),
        Err(e) => {
            tracing::error!(template = name, error = %e, "template render failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("template error: {e}"),
            )
                .into_response()
        }
    }
}

pub(crate) fn base_ctx(session_user: Option<&str>) -> Context {
    let mut ctx = Context::new();
    ctx.insert("authed", &session_user.is_some());
    if let Some(u) = session_user {
        ctx.insert("user", u);
    }
    ctx
}

/// Build a page context with user + flash (consumes flash). Preferred over
/// `base_ctx` for authenticated views so POST-redirect-GET messages surface.
pub(crate) async fn page_ctx(session: &Session) -> Context {
    let user = current_user(session).await;
    let mut ctx = base_ctx(user.as_deref());
    if let Some(msg) = actions::take_flash(session).await {
        ctx.insert("flash", &msg);
    }
    ctx
}

fn insecure_http() -> bool {
    std::env::var("MOSAICFS_INSECURE_HTTP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Build the `/admin` router. Returns a state-typed router to be merged
/// before the main router applies its state.
pub fn router() -> Router<Arc<AppState>> {
    let store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(store)
        .with_name("mosaicfs_admin")
        .with_same_site(SameSite::Lax)
        .with_secure(!insecure_http())
        .with_expiry(tower_sessions::Expiry::OnInactivity(Duration::hours(12)));

    let public: Router<Arc<AppState>> = Router::new()
        .route("/admin/login", get(login_form).post(login_submit))
        .route("/admin/logout", post(logout))
        .route(
            "/admin/bootstrap",
            get(actions::bootstrap_page).post(actions::bootstrap_submit),
        )
        .route("/admin/assets/{*path}", get(serve_asset));

    let protected: Router<Arc<AppState>> = Router::new()
        .route("/admin", get(|| async { Redirect::to("/admin/browse") }))
        .route("/admin/browse", get(views::browse_page))
        .route("/admin/status", get(views::status_page))
        .route("/admin/status/panel", get(views::status_panel))
        .route("/admin/nodes", get(views::nodes_page))
        .route("/admin/nodes/panel", get(views::nodes_panel))
        .route("/admin/nodes/{node_id}", get(views::node_detail_page))
        .route("/admin/nodes/{node_id}/edit", post(actions::patch_node_action))
        .route("/admin/nodes/{node_id}/mounts", post(actions::add_mount_action))
        .route(
            "/admin/nodes/{node_id}/mounts/{mount_id}/delete",
            post(actions::delete_mount_action),
        )
        .route("/admin/notifications", get(views::notifications_page))
        .route(
            "/admin/notifications/panel",
            get(views::notifications_panel),
        )
        .route(
            "/admin/notifications/ack-all",
            post(actions::ack_all_notifications),
        )
        .route(
            "/admin/notifications/{id}/ack",
            post(actions::ack_notification),
        )
        .route("/admin/replication", get(views::replication_page))
        .route("/admin/replication/panel", get(views::replication_panel))
        .route(
            "/admin/replication/rules/create",
            post(actions::create_rule_action),
        )
        .route(
            "/admin/replication/rules/{rule_id}/delete",
            post(actions::delete_rule_action),
        )
        .route(
            "/admin/replication/restore",
            post(actions::initiate_restore_action),
        )
        .route(
            "/admin/replication/restore/{job_id}/cancel",
            post(actions::cancel_restore_action),
        )
        .route(
            "/admin/storage-backends",
            get(views::storage_backends_page),
        )
        .route(
            "/admin/storage-backends/create",
            post(actions::create_backend_action),
        )
        .route(
            "/admin/storage-backends/{name}/delete",
            post(actions::delete_backend_action),
        )
        .route(
            "/admin/settings/credentials",
            get(views::settings_credentials_page),
        )
        .route(
            "/admin/settings/credentials/create",
            post(actions::create_credential_action),
        )
        .route(
            "/admin/settings/credentials/{key_id}/delete",
            post(actions::delete_credential_action),
        )
        .route(
            "/admin/settings/credentials/{key_id}/toggle",
            post(actions::toggle_credential_action),
        )
        .route("/admin/settings/backup", get(views::settings_backup_page))
        .route("/admin/settings/backup/download", get(actions::backup_download))
        .route("/admin/vfs", get(views::vfs_page))
        .route("/admin/vfs/new", get(views::vfs_new_page))
        .route("/admin/vfs/dir", get(views::vfs_dir_page))
        .route("/admin/vfs/dir/create", post(actions::create_vfs_dir_action))
        .route("/admin/vfs/dir/delete", post(actions::delete_vfs_dir_action))
        .route("/admin/vfs/dir/settings", post(actions::patch_vfs_dir_action))
        .route("/admin/vfs/dir/mounts/add", post(actions::add_vfs_mount_action))
        .route("/admin/vfs/dir/mounts/delete", post(actions::delete_vfs_mount_action))
        .route("/admin/vfs/dir/mounts/steps/add", post(actions::add_vfs_step_action))
        .route("/admin/vfs/dir/mounts/steps/delete", post(actions::delete_vfs_step_action))
        .route("/admin/vfs/dir/mounts/steps/move", post(actions::move_vfs_step_action))
        .route("/admin/vfs/open-file", post(actions::open_file_action))
        .layer(middleware::from_fn(require_auth));

    Router::new().merge(public).merge(protected).layer(session_layer)
}

async fn require_auth(session: Session, req: Request, next: Next) -> Response {
    if insecure_http() {
        return next.run(req).await;
    }
    match session.get::<String>(SESSION_USER_KEY).await {
        Ok(Some(_)) => next.run(req).await,
        _ => {
            let path = req.uri().path();
            if path.starts_with("/admin/") && path.ends_with("/panel") {
                // HTMX poll for an unauthed user: return a gentle 401 body that HTMX will swap.
                return (StatusCode::UNAUTHORIZED, "Session expired. Reload.")
                    .into_response();
            }
            Redirect::to("/admin/login").into_response()
        }
    }
}

async fn current_user(session: &Session) -> Option<String> {
    if insecure_http() {
        return Some("insecure-http".to_string());
    }
    session.get::<String>(SESSION_USER_KEY).await.ok().flatten()
}

pub(crate) async fn user_for_ctx(session: &Session) -> Option<String> {
    current_user(session).await
}

async fn login_form(State(state): State<Arc<AppState>>, session: Session) -> Response {
    if current_user(&session).await.is_some() {
        return Redirect::to("/admin/status").into_response();
    }
    if state.data_dir.join("bootstrap_token").exists() {
        return Redirect::to("/admin/bootstrap").into_response();
    }
    let ctx = base_ctx(None);
    render("login.html", &ctx)
}

#[derive(Deserialize)]
struct LoginForm {
    access_key_id: String,
    secret_key: String,
}

async fn login_submit(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> Response {
    let doc = match state
        .db
        .get_document(&format!("credential::{}", form.access_key_id))
        .await
    {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => return login_error("Invalid credentials"),
        Err(e) => {
            tracing::error!(error=%e, "admin login: couch error");
            return login_error("Internal error");
        }
    };

    if doc.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return login_error("Invalid credentials");
    }
    let hash = doc
        .get("secret_key_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !credentials::verify_secret(&form.secret_key, hash) {
        return login_error("Invalid credentials");
    }

    if let Err(e) = session
        .insert(SESSION_USER_KEY, &form.access_key_id)
        .await
    {
        tracing::error!(error=%e, "session insert failed");
        return login_error("Session error");
    }
    Redirect::to("/admin/status").into_response()
}

fn login_error(msg: &str) -> Response {
    let mut ctx = base_ctx(None);
    ctx.insert("error", msg);
    (StatusCode::UNAUTHORIZED, render("login.html", &ctx)).into_response()
}

async fn logout(session: Session) -> Response {
    let _ = session.flush().await;
    Redirect::to("/admin/login").into_response()
}

async fn serve_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches("/admin/assets/");
    let (bytes, content_type): (&[u8], &str) = match path {
        "pico.min.css" => (
            include_bytes!("../../assets/pico.min.css"),
            "text/css; charset=utf-8",
        ),
        "htmx.min.js" => (
            include_bytes!("../../assets/htmx.min.js"),
            "application/javascript; charset=utf-8",
        ),
        _ => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    )
        .into_response()
}
