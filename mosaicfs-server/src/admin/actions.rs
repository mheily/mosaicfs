//! Write handlers for the admin UI (Phase 3).
//!
//! Pattern: each handler parses an axum `Form<T>`, performs a CouchDB write
//! (direct, not through the REST handlers — those return JSON), stuffs a
//! one-shot flash message into the session, and redirects (303) back to the
//! relevant list page. The GET handler reads-and-clears `_flash` and
//! surfaces it via the layout `flash` variable.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Form,
};
use chrono::Utc;
use serde::Deserialize;
use tera::Context;
use tower_sessions::Session;
use uuid::Uuid;

use crate::admin::{base_ctx, render, user_for_ctx, FLASH_KEY, NEW_SECRET_KEY};
use crate::credentials;
use crate::handlers::{replication as rephandlers, system as syshandlers};
use crate::state::AppState;
use mosaicfs_common::couchdb::CouchError;

pub(crate) async fn set_flash(session: &Session, msg: impl Into<String>) {
    let _ = session.insert(FLASH_KEY, &msg.into()).await;
}

pub(crate) async fn take_flash(session: &Session) -> Option<String> {
    let msg: Option<String> = session.remove(FLASH_KEY).await.ok().flatten();
    msg
}

fn redirect(path: &str) -> Response {
    Redirect::to(path).into_response()
}

// ── Bootstrap ──

#[derive(Deserialize)]
pub struct BootstrapForm {
    pub token: String,
}

pub async fn bootstrap_page(State(state): State<Arc<AppState>>, session: Session) -> Response {
    let token_path = state.data_dir.join("bootstrap_token");
    if !token_path.exists() {
        return redirect("/admin/login");
    }
    let new_secret: Option<(String, String)> = session
        .remove::<(String, String)>(NEW_SECRET_KEY)
        .await
        .ok()
        .flatten();
    let mut ctx = base_ctx(None);
    ctx.insert("title", "Bootstrap — MosaicFS");
    if let Some((ak, sk)) = new_secret {
        ctx.insert("created_access_key", &ak);
        ctx.insert("created_secret_key", &sk);
    }
    render("bootstrap.html", &ctx)
}

pub async fn bootstrap_submit(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<BootstrapForm>,
) -> Response {
    let token_path = state.data_dir.join("bootstrap_token");
    let stored = match std::fs::read_to_string(&token_path) {
        Ok(t) => t.trim().to_string(),
        Err(_) => {
            return bootstrap_error(&session, "Bootstrap is not required.").await;
        }
    };
    if form.token.trim() != stored {
        return bootstrap_error(&session, "Invalid bootstrap token.").await;
    }
    match credentials::create_credential(&state.db, "admin").await {
        Ok((ak, sk)) => {
            let _ = std::fs::remove_file(&token_path);
            let _ = session
                .insert(NEW_SECRET_KEY, &(ak.clone(), sk.clone()))
                .await;
            redirect("/admin/bootstrap")
        }
        Err(e) => {
            tracing::error!(error=%e, "bootstrap: create_credential failed");
            bootstrap_error(&session, "Failed to create credential.").await
        }
    }
}

async fn bootstrap_error(session: &Session, msg: &str) -> Response {
    set_flash(session, msg).await;
    redirect("/admin/bootstrap")
}

// ── Notifications ──

pub async fn ack_notification(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(id): Path<String>,
) -> Response {
    let doc_id = if id.starts_with("notification::") {
        id.clone()
    } else {
        format!("notification::{}", id)
    };
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            set_flash(&session, "Notification not found.").await;
            return redirect("/admin/notifications");
        }
        Err(e) => {
            set_flash(&session, format!("Error: {e}")).await;
            return redirect("/admin/notifications");
        }
    };
    doc["status"] = serde_json::Value::String("acknowledged".to_string());
    doc["acknowledged_at"] = serde_json::Value::String(Utc::now().to_rfc3339());
    if let Err(e) = state.db.put_document(&doc_id, &doc).await {
        set_flash(&session, format!("Write failed: {e}")).await;
    } else {
        set_flash(&session, "Notification acknowledged.").await;
    }
    redirect("/admin/notifications")
}

pub async fn ack_all_notifications(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Response {
    let resp = match state.db.all_docs_by_prefix("notification::", true).await {
        Ok(r) => r,
        Err(e) => {
            set_flash(&session, format!("Read failed: {e}")).await;
            return redirect("/admin/notifications");
        }
    };
    let now = Utc::now().to_rfc3339();
    let mut batch: Vec<serde_json::Value> = Vec::new();
    for row in resp.rows {
        if let Some(mut doc) = row.doc {
            if doc.get("type").and_then(|v| v.as_str()) != Some("notification") {
                continue;
            }
            if doc.get("status").and_then(|v| v.as_str()).unwrap_or("active") != "active" {
                continue;
            }
            doc["status"] = serde_json::Value::String("acknowledged".to_string());
            doc["acknowledged_at"] = serde_json::Value::String(now.clone());
            batch.push(doc);
        }
    }
    let n = batch.len();
    if !batch.is_empty() {
        if let Err(e) = state.db.bulk_docs(&batch).await {
            set_flash(&session, format!("Write failed: {e}")).await;
            return redirect("/admin/notifications");
        }
    }
    set_flash(&session, format!("Acknowledged {n} notification(s).")).await;
    redirect("/admin/notifications")
}

// ── Credentials ──

#[derive(Deserialize)]
pub struct CreateCredentialForm {
    pub name: String,
}

pub async fn create_credential_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<CreateCredentialForm>,
) -> Response {
    let name = form.name.trim();
    if name.is_empty() {
        set_flash(&session, "Credential name is required.").await;
        return redirect("/admin/settings/credentials");
    }
    match credentials::create_credential(&state.db, name).await {
        Ok((ak, sk)) => {
            let _ = session.insert(NEW_SECRET_KEY, &(ak, sk)).await;
            set_flash(&session, "Credential created. Copy the secret key — it is shown only once.").await;
        }
        Err(e) => {
            set_flash(&session, format!("Create failed: {e}")).await;
        }
    }
    redirect("/admin/settings/credentials")
}

pub async fn delete_credential_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(key_id): Path<String>,
) -> Response {
    match credentials::delete_credential(&state.db, &key_id).await {
        Ok(()) => set_flash(&session, format!("Credential {key_id} deleted.")).await,
        Err(e) => set_flash(&session, format!("Delete failed: {e}")).await,
    }
    redirect("/admin/settings/credentials")
}

#[derive(Deserialize)]
pub struct ToggleCredentialForm {
    pub enabled: Option<String>,
}

pub async fn toggle_credential_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(key_id): Path<String>,
    Form(form): Form<ToggleCredentialForm>,
) -> Response {
    let enabled = form.enabled.as_deref() == Some("1");
    let update = serde_json::json!({ "enabled": enabled });
    match credentials::update_credential(&state.db, &key_id, &update).await {
        Ok(()) => {
            let label = if enabled { "enabled" } else { "disabled" };
            set_flash(&session, format!("Credential {key_id} {label}.")).await;
        }
        Err(e) => set_flash(&session, format!("Update failed: {e}")).await,
    }
    redirect("/admin/settings/credentials")
}

// ── Nodes ──

#[derive(Deserialize)]
pub struct PatchNodeForm {
    pub friendly_name: String,
}

pub async fn patch_node_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(node_id): Path<String>,
    Form(form): Form<PatchNodeForm>,
) -> Response {
    let doc_id = format!("node::{}", node_id);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => {
            set_flash(&session, "Node not found.").await;
            return redirect("/admin/nodes");
        }
    };
    doc["friendly_name"] = serde_json::Value::String(form.friendly_name.trim().to_string());
    if let Err(e) = state.db.put_document(&doc_id, &doc).await {
        set_flash(&session, format!("Update failed: {e}")).await;
    } else {
        set_flash(&session, "Node updated.").await;
    }
    redirect(&format!("/admin/nodes/{}", node_id))
}

#[derive(Deserialize)]
pub struct AddMountForm {
    pub remote_node_id: String,
    pub remote_base_export_path: String,
    pub local_mount_path: String,
    pub mount_type: String,
    pub priority: Option<String>,
}

pub async fn add_mount_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(node_id): Path<String>,
    Form(form): Form<AddMountForm>,
) -> Response {
    let doc_id = format!("node::{}", node_id);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => {
            set_flash(&session, "Node not found.").await;
            return redirect("/admin/nodes");
        }
    };
    let priority: i32 = form.priority.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mount_id: String = Uuid::new_v4().to_string().chars().take(8).collect();
    let mount = serde_json::json!({
        "mount_id": mount_id,
        "remote_node_id": form.remote_node_id.trim(),
        "remote_base_export_path": form.remote_base_export_path.trim(),
        "local_mount_path": form.local_mount_path.trim(),
        "mount_type": form.mount_type.trim(),
        "priority": priority,
    });
    let mounts = doc
        .as_object_mut()
        .and_then(|o| Some(o.entry("network_mounts").or_insert_with(|| serde_json::json!([]))));
    if let Some(m) = mounts {
        if let Some(arr) = m.as_array_mut() {
            arr.push(mount);
        }
    }
    if let Err(e) = state.db.put_document(&doc_id, &doc).await {
        set_flash(&session, format!("Write failed: {e}")).await;
    } else {
        set_flash(&session, "Mount added.").await;
    }
    redirect(&format!("/admin/nodes/{}", node_id))
}

pub async fn delete_mount_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path((node_id, mount_id)): Path<(String, String)>,
) -> Response {
    let doc_id = format!("node::{}", node_id);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => {
            set_flash(&session, "Node not found.").await;
            return redirect("/admin/nodes");
        }
    };
    if let Some(arr) = doc.get_mut("network_mounts").and_then(|v| v.as_array_mut()) {
        arr.retain(|m| m.get("mount_id").and_then(|v| v.as_str()) != Some(&mount_id));
    }
    if let Err(e) = state.db.put_document(&doc_id, &doc).await {
        set_flash(&session, format!("Write failed: {e}")).await;
    } else {
        set_flash(&session, "Mount removed.").await;
    }
    redirect(&format!("/admin/nodes/{}", node_id))
}

// ── Storage backends ──

#[derive(Deserialize)]
pub struct CreateBackendForm {
    pub name: String,
    pub backend: String, // s3, b2, directory, agent
    pub path_or_bucket: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub credentials_ref: Option<String>,
    pub enabled: Option<String>,
}

pub async fn create_backend_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<CreateBackendForm>,
) -> Response {
    let name = form.name.trim().to_string();
    if name.is_empty() || name.contains("::") {
        set_flash(&session, "Invalid backend name.").await;
        return redirect("/admin/storage-backends");
    }
    let valid = ["s3", "b2", "directory", "agent"];
    if !valid.contains(&form.backend.as_str()) {
        set_flash(&session, "Backend type must be one of: s3, b2, directory, agent.").await;
        return redirect("/admin/storage-backends");
    }

    let doc_id = mosaicfs_common::replication::storage_backend_doc_id(&name);
    if state.db.get_document(&doc_id).await.is_ok() {
        set_flash(&session, "A backend with that name already exists.").await;
        return redirect("/admin/storage-backends");
    }

    let backend_config = match form.backend.as_str() {
        "directory" => serde_json::json!({ "path": form.path_or_bucket.unwrap_or_default() }),
        "s3" => serde_json::json!({
            "bucket": form.path_or_bucket.unwrap_or_default(),
            "endpoint": form.endpoint.unwrap_or_default(),
            "region": form.region.unwrap_or_default(),
        }),
        "b2" => serde_json::json!({
            "bucket": form.path_or_bucket.unwrap_or_default(),
        }),
        "agent" => serde_json::json!({}),
        _ => serde_json::json!({}),
    };

    let doc = serde_json::json!({
        "_id": doc_id,
        "type": "storage_backend",
        "name": name,
        "backend": form.backend,
        "mode": "target",
        "backend_config": backend_config,
        "credentials_ref": form.credentials_ref,
        "retention": { "keep_deleted_days": 30 },
        "remove_unmatched": false,
        "enabled": form.enabled.as_deref() == Some("1"),
        "created_at": Utc::now().to_rfc3339(),
    });

    match state
        .db
        .put_document(&mosaicfs_common::replication::storage_backend_doc_id(&name), &doc)
        .await
    {
        Ok(_) => set_flash(&session, format!("Backend '{name}' created.")).await,
        Err(e) => set_flash(&session, format!("Create failed: {e}")).await,
    }
    redirect("/admin/storage-backends")
}

pub async fn delete_backend_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(name): Path<String>,
) -> Response {
    let doc_id = mosaicfs_common::replication::storage_backend_doc_id(&name);
    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => {
            set_flash(&session, "Backend not found.").await;
            return redirect("/admin/storage-backends");
        }
    };
    let rev = doc.get("_rev").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if rev.is_empty() {
        set_flash(&session, "Backend has no _rev (corrupt doc).").await;
        return redirect("/admin/storage-backends");
    }
    match state.db.delete_document(&doc_id, &rev).await {
        Ok(_) => set_flash(&session, format!("Backend '{name}' deleted.")).await,
        Err(e) => set_flash(&session, format!("Delete failed: {e}")).await,
    }
    redirect("/admin/storage-backends")
}

// ── Replication rules ──

#[derive(Deserialize)]
pub struct CreateRuleForm {
    pub name: Option<String>,
    pub target_name: String,
    pub source_node_id: Option<String>,
    pub default_result: Option<String>, // "include" | "exclude"
    pub enabled: Option<String>,
}

pub async fn create_rule_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<CreateRuleForm>,
) -> Response {
    if form.target_name.trim().is_empty() {
        set_flash(&session, "Target backend is required.").await;
        return redirect("/admin/replication");
    }
    let rule_id = Uuid::new_v4().to_string();
    let name = form
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("rule-{}", &rule_id[..8]));
    let source_node = form
        .source_node_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "*".to_string());

    let doc_id = format!("replication_rule::{}", rule_id);
    let doc = serde_json::json!({
        "_id": doc_id,
        "type": "replication_rule",
        "rule_id": rule_id,
        "name": name,
        "target_name": form.target_name.trim(),
        "source": { "node_id": source_node },
        "steps": [],
        "default_result": form.default_result.as_deref().unwrap_or("exclude"),
        "enabled": form.enabled.as_deref() == Some("1"),
        "created_at": Utc::now().to_rfc3339(),
        "updated_at": Utc::now().to_rfc3339(),
    });
    match state
        .db
        .put_document(&format!("replication_rule::{}", rule_id), &doc)
        .await
    {
        Ok(_) => set_flash(&session, "Replication rule created.").await,
        Err(e) => set_flash(&session, format!("Create failed: {e}")).await,
    }
    redirect("/admin/replication")
}

pub async fn delete_rule_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(rule_id): Path<String>,
) -> Response {
    let doc_id = format!("replication_rule::{}", rule_id);
    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(_) => {
            set_flash(&session, "Rule not found.").await;
            return redirect("/admin/replication");
        }
    };
    let rev = doc.get("_rev").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if rev.is_empty() {
        set_flash(&session, "Rule has no _rev.").await;
        return redirect("/admin/replication");
    }
    match state.db.delete_document(&doc_id, &rev).await {
        Ok(_) => set_flash(&session, "Rule deleted.").await,
        Err(e) => set_flash(&session, format!("Delete failed: {e}")).await,
    }
    redirect("/admin/replication")
}

// ── Restore ──

#[derive(Deserialize)]
pub struct RestoreForm {
    pub target_name: String,
    pub source_node_id: String,
    pub destination_node_id: String,
    pub destination_path: Option<String>,
    pub path_prefix: Option<String>,
    pub mime_type: Option<String>,
}

pub async fn initiate_restore_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<RestoreForm>,
) -> Response {
    let empty = |s: &str| s.trim().is_empty();
    if empty(&form.target_name) || empty(&form.source_node_id) || empty(&form.destination_node_id) {
        set_flash(&session, "target_name, source_node_id, destination_node_id are required.").await;
        return redirect("/admin/replication");
    }
    let body = rephandlers::InitiateRestoreRequest {
        target_name: form.target_name.trim().to_string(),
        source_node_id: form.source_node_id.trim().to_string(),
        destination_node_id: form.destination_node_id.trim().to_string(),
        destination_path: form.destination_path.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        path_prefix: form.path_prefix.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        mime_type: form.mime_type.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    };
    let resp = rephandlers::initiate_restore(State(state), axum::Json(body))
        .await
        .into_response();
    match resp.status() {
        StatusCode::ACCEPTED => {
            set_flash(&session, "Restore job started.").await;
        }
        s => {
            set_flash(&session, format!("Restore failed: {s}")).await;
        }
    }
    redirect("/admin/replication")
}

pub async fn cancel_restore_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(job_id): Path<String>,
) -> Response {
    let outcome: &str = {
        let mut jobs = state.restore_jobs.lock().unwrap();
        match jobs.get_mut(&job_id) {
            Some(job) if job.status == "running" => {
                job.status = "cancelled".to_string();
                job.completed_at = Some(Utc::now().to_rfc3339());
                "cancelled"
            }
            Some(_) => "not_running",
            None => "not_found",
        }
    };
    match outcome {
        "cancelled" => set_flash(&session, "Restore job cancelled.").await,
        "not_running" => set_flash(&session, "Job not running.").await,
        _ => set_flash(&session, "Job not found.").await,
    }
    redirect("/admin/replication")
}

// ── Backup download ──

pub async fn backup_download(
    State(state): State<Arc<AppState>>,
    Query(query): Query<syshandlers::BackupQuery>,
) -> Response {
    syshandlers::backup(State(state), Query(query)).await.into_response()
}
