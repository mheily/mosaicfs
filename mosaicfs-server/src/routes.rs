use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{any, delete, get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};

use crate::auth::hmac_auth::{self, HmacClaims};
use crate::auth::jwt::{self, Claims};
use crate::credentials;
use crate::couchdb::CouchError;
use crate::handlers::{agent, files, labels, nodes, notifications, replication, search, system, vfs};
use crate::state::AppState;

const LOGIN_RATE_LIMIT: u32 = 5;
const LOGIN_RATE_WINDOW_SECS: u64 = 60;

pub fn build_router(state: Arc<AppState>) -> Router {
    let jwt_routes = Router::new()
        // Auth
        .route("/api/auth/whoami", get(whoami))
        .route("/api/auth/logout", post(logout))
        // Credentials
        .route("/api/credentials", get(list_credentials))
        .route("/api/credentials", post(create_credential))
        .route("/api/credentials/{key_id}", get(get_credential))
        .route("/api/credentials/{key_id}", patch(update_credential))
        .route("/api/credentials/{key_id}", delete(delete_credential))
        // Nodes
        .route("/api/nodes", get(nodes::list_nodes))
        .route("/api/nodes", post(nodes::register_node))
        .route("/api/nodes/{node_id}", get(nodes::get_node))
        .route("/api/nodes/{node_id}", patch(nodes::patch_node))
        .route("/api/nodes/{node_id}", delete(nodes::delete_node))
        .route("/api/nodes/{node_id}/status", get(nodes::get_node_status))
        .route("/api/nodes/{node_id}/files", get(stub_501))
        .route("/api/nodes/{node_id}/storage", get(stub_501))
        .route("/api/nodes/{node_id}/utilization", get(stub_501))
        .route("/api/nodes/{node_id}/mounts", get(nodes::list_mounts))
        .route("/api/nodes/{node_id}/mounts", post(nodes::add_mount))
        .route("/api/nodes/{node_id}/mounts/{mount_id}", patch(nodes::patch_mount))
        .route("/api/nodes/{node_id}/mounts/{mount_id}", delete(nodes::delete_mount))
        // Files
        .route("/api/files", get(files::list_files))
        .route("/api/files/by-path", get(files::get_file_by_path))
        .route("/api/files/{file_id}", get(files::get_file))
        .route("/api/files/{file_id}/content", get(files::get_file_content))
        .route("/api/files/{file_id}/access", get(files::get_file_access))
        // VFS
        .route("/api/vfs", get(vfs::list_vfs))
        .route("/api/vfs/tree", get(vfs::get_tree))
        .route("/api/vfs/directories", post(vfs::create_directory))
        .route("/api/vfs/directories/{*path}", get(vfs::get_directory))
        .route("/api/vfs/directories/{*path}", patch(vfs::patch_directory))
        .route("/api/vfs/directories/{*path}", delete(vfs::delete_directory))
        .route("/api/vfs/preview/{*path}", post(vfs::preview_directory))
        // Search
        .route("/api/search", get(search::search))
        // Labels
        .route("/api/labels", get(labels::list_labels))
        .route("/api/labels/assignments", get(labels::list_assignments))
        .route("/api/labels/assignments", put(labels::upsert_assignment))
        .route("/api/labels/assignments", delete(labels::delete_assignment))
        .route("/api/labels/rules", get(labels::list_rules))
        .route("/api/labels/rules", post(labels::create_rule))
        .route("/api/labels/rules/{rule_id}", patch(labels::patch_rule))
        .route("/api/labels/rules/{rule_id}", delete(labels::delete_rule))
        .route("/api/labels/effective", get(labels::effective_labels))
        // Storage Backends
        .route("/api/storage-backends", get(replication::list_storage_backends))
        .route("/api/storage-backends", post(replication::create_storage_backend))
        .route("/api/storage-backends/{name}", get(replication::get_storage_backend))
        .route("/api/storage-backends/{name}", patch(replication::patch_storage_backend))
        .route("/api/storage-backends/{name}", delete(replication::delete_storage_backend))
        // Replication Rules
        .route("/api/replication/rules", get(replication::list_replication_rules))
        .route("/api/replication/rules", post(replication::create_replication_rule))
        .route("/api/replication/rules/{rule_id}", get(replication::get_replication_rule))
        .route("/api/replication/rules/{rule_id}", patch(replication::patch_replication_rule))
        .route("/api/replication/rules/{rule_id}", delete(replication::delete_replication_rule))
        // Replicas and status
        .route("/api/replication/replicas", get(replication::list_replicas))
        .route("/api/replication/status", get(replication::get_replication_status))
        // Restore operations
        .route("/api/replication/restore", post(replication::initiate_restore))
        .route("/api/replication/restore/history", get(replication::list_restore_history))
        .route("/api/replication/restore/{job_id}", get(replication::get_restore_job))
        .route("/api/replication/restore/{job_id}/cancel", post(replication::cancel_restore_job))
        // Notifications
        .route("/api/notifications", get(notifications::list_notifications))
        .route("/api/notifications/{id}/acknowledge", post(notifications::acknowledge_notification))
        .route("/api/notifications/acknowledge-all", post(notifications::acknowledge_all))
        .route("/api/notifications/history", get(notifications::notification_history))
        // Annotations
        .route("/api/annotations", get(stub_501))
        .route("/api/annotations", delete(stub_501))
        // Plugins
        .route("/api/nodes/{node_id}/plugins", get(stub_501))
        .route("/api/nodes/{node_id}/plugins", post(stub_501))
        .route("/api/nodes/{node_id}/plugins/{plugin_name}", get(stub_501))
        .route("/api/nodes/{node_id}/plugins/{plugin_name}", patch(stub_501))
        .route("/api/nodes/{node_id}/plugins/{plugin_name}", delete(stub_501))
        .route("/api/nodes/{node_id}/plugins/{plugin_name}/sync", post(stub_501))
        // Query
        .route("/api/query", post(stub_501))
        // System
        .route("/api/health", get(system::health))
        .route("/api/system/info", get(system::system_info))
        .route("/api/system/backup", get(system::backup))
        .route("/api/system/backup/status", get(system::backup_status))
        .route("/api/system/restore", post(system::restore))
        .route("/api/system/data", delete(system::delete_data))
        .route("/api/storage", get(stub_501))
        .layer(middleware::from_fn_with_state(state.clone(), jwt::jwt_middleware));

    let hmac_routes = Router::new()
        .route("/api/agent/heartbeat", post(agent::heartbeat))
        .route("/api/agent/files/bulk", post(agent::bulk_files))
        .route("/api/agent/status", post(agent::agent_status))
        .route("/api/agent/utilization", post(agent::agent_utilization))
        .route("/api/agent/credentials", get(agent::agent_credentials))
        .route("/api/agent/transfer/{file_id}", get(agent::agent_transfer))
        .route("/api/agent/query", post(stub_501))
        .route("/api/agent/replicate/{*path}", any(agent::replicate_proxy))
        .layer(middleware::from_fn_with_state(state.clone(), hmac_auth::hmac_middleware));

    // Unauthenticated routes
    let public_routes = Router::new()
        .route("/api/auth/login", post(login));

    Router::new()
        .merge(public_routes)
        .merge(jwt_routes)
        .merge(hmac_routes)
        .fallback_service(
            ServeDir::new("web/dist")
                .fallback(ServeFile::new("web/dist/index.html")),
        )
        .with_state(state)
}

// ── Stub handler ──

async fn stub_501() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Not implemented yet")
}

// ── Auth handlers ──

#[derive(Deserialize)]
struct LoginRequest {
    access_key_id: String,
    secret_key: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    // Rate limiting
    {
        let mut attempts = state.login_attempts.lock().unwrap();
        let entry = attempts
            .entry(body.access_key_id.clone())
            .or_insert((0, Instant::now()));

        if entry.1.elapsed().as_secs() > LOGIN_RATE_WINDOW_SECS {
            *entry = (0, Instant::now());
        }

        if entry.0 >= LOGIN_RATE_LIMIT {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": { "code": "rate_limited", "message": "Too many login attempts" } })),
            );
        }
        entry.0 += 1;
    }

    // Look up credential
    let doc = match state
        .db
        .get_document(&format!("credential::{}", body.access_key_id))
        .await
    {
        Ok(doc) => doc,
        Err(CouchError::NotFound(_)) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": { "code": "unauthorized", "message": "Invalid credentials" } })),
            );
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": { "code": "internal", "message": "Internal error" } })),
            );
        }
    };

    if doc.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": { "code": "unauthorized", "message": "Invalid credentials" } })),
        );
    }

    let hash = doc
        .get("secret_key_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !credentials::verify_secret(&body.secret_key, hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": { "code": "unauthorized", "message": "Invalid credentials" } })),
        );
    }

    match jwt::issue_token(&state.jwt_secret, &body.access_key_id) {
        Ok((token, expires_at)) => {
            {
                let mut attempts = state.login_attempts.lock().unwrap();
                attempts.remove(&body.access_key_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "token": token,
                    "expires_at": expires_at,
                })),
            )
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": { "code": "internal", "message": "Failed to issue token" } })),
        ),
    }
}

async fn whoami(req: axum::extract::Request) -> impl IntoResponse {
    let claims = req.extensions().get::<Claims>().unwrap();
    Json(serde_json::json!({
        "access_key_id": claims.sub,
        "token_id": claims.jti,
        "issued_at": claims.iat,
        "expires_at": claims.exp,
    }))
}

async fn logout(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let claims = req.extensions().get::<Claims>().unwrap();
    let jti = claims.jti.clone();
    let mut revoked = state.revoked_tokens.lock().unwrap();
    revoked.push(jti);
    Json(serde_json::json!({ "ok": true }))
}

// ── Credential handlers ──

async fn list_credentials(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match credentials::list_credentials(&state.db).await {
        Ok(creds) => (StatusCode::OK, Json(serde_json::json!({ "items": creds, "total": creds.len() }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": { "code": "internal", "message": e.to_string() } })),
        ),
    }
}

#[derive(Deserialize)]
struct CreateCredentialRequest {
    name: String,
}

async fn create_credential(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateCredentialRequest>,
) -> impl IntoResponse {
    match credentials::create_credential(&state.db, &body.name).await {
        Ok((access_key_id, secret_key)) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "access_key_id": access_key_id,
                "secret_key": secret_key,
                "name": body.name,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": { "code": "internal", "message": e.to_string() } })),
        ),
    }
}

async fn get_credential(
    State(state): State<Arc<AppState>>,
    Path(key_id): Path<String>,
) -> impl IntoResponse {
    match credentials::get_credential(&state.db, &key_id).await {
        Ok(doc) => (StatusCode::OK, Json(doc)),
        Err(CouchError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": { "code": "not_found", "message": "Credential not found" } })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": { "code": "internal", "message": e.to_string() } })),
        ),
    }
}

async fn update_credential(
    State(state): State<Arc<AppState>>,
    Path(key_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match credentials::update_credential(&state.db, &key_id, &body).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": { "code": "internal", "message": e.to_string() } })),
        ),
    }
}

async fn delete_credential(
    State(state): State<Arc<AppState>>,
    Path(key_id): Path<String>,
) -> impl IntoResponse {
    match credentials::delete_credential(&state.db, &key_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": { "code": "internal", "message": e.to_string() } })),
        ),
    }
}
