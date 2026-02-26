use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::couchdb::CouchError;
use crate::state::AppState;

fn error_json(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "code": code, "message": message } })
}

fn strip_internals(doc: &mut serde_json::Value) {
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("_rev");
        if let Some(id) = obj.remove("_id") {
            obj.insert("id".to_string(), id);
        }
    }
}

// ── Storage Backends ──

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn list_storage_backends(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("storage_backend::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            if doc.get("type")?.as_str()? != "storage_backend" {
                return None;
            }
            // Mask credentials in list response
            strip_internals(&mut doc);
            Some(doc)
        })
        .collect();

    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(100) as usize;
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    (StatusCode::OK, Json(serde_json::json!({
        "items": items,
        "total": total,
        "offset": offset,
        "limit": limit,
    })))
}

#[derive(Deserialize)]
pub struct CreateStorageBackendRequest {
    pub name: String,
    #[serde(alias = "type")]
    pub backend: String, // "s3", "b2", "directory", "agent"
    #[serde(default = "default_mode")]
    pub mode: String,
    pub hosting_node_id: Option<String>,
    #[serde(alias = "config")]
    pub backend_config: serde_json::Value,
    pub credentials_ref: Option<String>,
    pub schedule: Option<String>,
    pub bandwidth_limit_mbps: Option<i32>,
    #[serde(default)]
    pub retention: serde_json::Value,
    #[serde(default)]
    pub remove_unmatched: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_mode() -> String { "target".to_string() }
fn default_enabled() -> bool { true }

pub async fn create_storage_backend(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateStorageBackendRequest>,
) -> impl IntoResponse {
    // Validate name
    if body.name.is_empty() || body.name.contains("::") {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_json("validation", "Invalid backend name")),
        );
    }

    // Only target mode in v1
    if body.mode != "target" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_json("validation", "Only mode='target' is supported in v1")),
        );
    }

    let valid_backends = ["s3", "b2", "directory", "agent"];
    if !valid_backends.contains(&body.backend.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_json("validation", "backend must be one of: s3, b2, directory, agent")),
        );
    }

    let doc_id = format!("storage_backend::{}", body.name);

    // Check for duplicate
    if state.db.get_document(&doc_id).await.is_ok() {
        return (
            StatusCode::CONFLICT,
            Json(error_json("conflict", "Storage backend with this name already exists")),
        );
    }

    let retention = if body.retention.is_null() {
        serde_json::json!({ "keep_deleted_days": 30 })
    } else {
        body.retention
    };

    let doc = serde_json::json!({
        "_id": doc_id,
        "type": "storage_backend",
        "name": body.name,
        "backend": body.backend,
        "mode": body.mode,
        "hosting_node_id": body.hosting_node_id,
        "backend_config": body.backend_config,
        "credentials_ref": body.credentials_ref,
        "schedule": body.schedule,
        "bandwidth_limit_mbps": body.bandwidth_limit_mbps,
        "retention": retention,
        "remove_unmatched": body.remove_unmatched,
        "enabled": body.enabled,
        "created_at": Utc::now().to_rfc3339(),
    });

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({ "name": body.name }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn get_storage_backend(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let doc_id = format!("storage_backend::{}", name);
    match state.db.get_document(&doc_id).await {
        Ok(mut doc) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "Storage backend not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

pub async fn patch_storage_backend(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let doc_id = format!("storage_backend::{}", name);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Storage backend not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    // Apply allowed patch fields
    let patchable = [
        "backend_config", "credentials_ref", "schedule", "bandwidth_limit_mbps",
        "retention", "remove_unmatched", "enabled", "hosting_node_id",
    ];
    if let Some(obj) = body.as_object() {
        for key in patchable {
            if let Some(v) = obj.get(key) {
                doc[key] = v.clone();
            }
        }
    }
    doc["updated_at"] = serde_json::Value::String(Utc::now().to_rfc3339());

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn delete_storage_backend(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let doc_id = format!("storage_backend::{}", name);
    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Storage backend not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let rev = match doc.get("_rev").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", "Missing _rev"))),
    };

    match state.db.delete_document(&doc_id, &rev).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

// ── Replication Rules ──

pub async fn list_replication_rules(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("replication_rule::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            if doc.get("type")?.as_str()? != "replication_rule" {
                return None;
            }
            strip_internals(&mut doc);
            Some(doc)
        })
        .collect();

    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(100) as usize;
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    (StatusCode::OK, Json(serde_json::json!({
        "items": items,
        "total": total,
        "offset": offset,
        "limit": limit,
    })))
}

#[derive(Deserialize)]
pub struct CreateReplicationRuleRequest {
    pub name: Option<String>,
    #[serde(alias = "target")]
    pub target_name: String,
    // Accept either a source object or a flat source_node_id string
    pub source: Option<serde_json::Value>,
    pub source_node_id: Option<String>,
    #[serde(default)]
    pub steps: Vec<serde_json::Value>,
    #[serde(default = "default_result")]
    pub default_result: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_result() -> String { "exclude".to_string() }

pub async fn create_replication_rule(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateReplicationRuleRequest>,
) -> impl IntoResponse {
    if body.target_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_json("validation", "target_name is required")),
        );
    }

    let rule_id = Uuid::new_v4().to_string();
    let name = body.name.unwrap_or_else(|| format!("rule-{}", &rule_id[..8]));

    // Build source object from either the nested format or the flat source_node_id field
    let source = match body.source.filter(|v| !v.is_null()) {
        Some(s) => s,
        None => {
            let nid = body.source_node_id.as_deref().unwrap_or("*");
            serde_json::json!({ "node_id": nid })
        }
    };

    let doc_id = format!("replication_rule::{}", rule_id);

    let doc = serde_json::json!({
        "_id": doc_id,
        "type": "replication_rule",
        "rule_id": rule_id,
        "name": name,
        "target_name": body.target_name,
        "source": source,
        "steps": body.steps,
        "default_result": body.default_result,
        "enabled": body.enabled,
        "created_at": Utc::now().to_rfc3339(),
        "updated_at": Utc::now().to_rfc3339(),
    });

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({ "rule_id": rule_id }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn get_replication_rule(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
) -> impl IntoResponse {
    let doc_id = format!("replication_rule::{}", rule_id);
    match state.db.get_document(&doc_id).await {
        Ok(mut doc) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "Replication rule not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

pub async fn patch_replication_rule(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let doc_id = format!("replication_rule::{}", rule_id);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Replication rule not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let patchable = ["name", "target_name", "source", "steps", "default_result", "enabled"];
    if let Some(obj) = body.as_object() {
        for key in patchable {
            if let Some(v) = obj.get(key) {
                doc[key] = v.clone();
            }
        }
    }
    doc["updated_at"] = serde_json::Value::String(Utc::now().to_rfc3339());

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn delete_replication_rule(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
) -> impl IntoResponse {
    let doc_id = format!("replication_rule::{}", rule_id);
    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Replication rule not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let rev = match doc.get("_rev").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", "Missing _rev"))),
    };

    match state.db.delete_document(&doc_id, &rev).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

// ── Replicas ──

#[derive(Deserialize, Default)]
pub struct ListReplicasQuery {
    pub file_id: Option<String>,
    #[serde(alias = "target")]
    pub target_name: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn list_replicas(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListReplicasQuery>,
) -> impl IntoResponse {
    let prefix = if let Some(ref file_id) = query.file_id {
        let uuid = file_id.strip_prefix("file::").unwrap_or(file_id);
        format!("replica::{}", uuid)
    } else {
        "replica::".to_string()
    };

    let resp = match state.db.all_docs_by_prefix(&prefix, true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            if doc.get("type")?.as_str()? != "replica" {
                return None;
            }
            if let Some(ref target) = query.target_name {
                if doc.get("target_name")?.as_str()? != target {
                    return None;
                }
            }
            if let Some(ref status) = query.status {
                if doc.get("status")?.as_str()? != status {
                    return None;
                }
            }
            strip_internals(&mut doc);
            Some(doc)
        })
        .collect();

    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(100) as usize;
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    (StatusCode::OK, Json(serde_json::json!({
        "items": items,
        "total": total,
        "offset": offset,
        "limit": limit,
    })))
}

// ── Replication Status ──

pub async fn get_replication_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Aggregate replica counts by status
    let resp = match state.db.all_docs_by_prefix("replica::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let mut current = 0u64;
    let mut stale = 0u64;
    let mut frozen = 0u64;
    let mut by_target: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for row in resp.rows {
        let doc = match row.doc {
            Some(d) => d,
            None => continue,
        };
        if doc.get("type").and_then(|v| v.as_str()) != Some("replica") {
            continue;
        }
        let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
        let target = doc.get("target_name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

        match status {
            "current" => current += 1,
            "stale" => stale += 1,
            "frozen" => frozen += 1,
            _ => {}
        }
        *by_target.entry(target).or_insert(0) += 1;
    }

    (StatusCode::OK, Json(serde_json::json!({
        "total_replicas": current + stale + frozen,
        "current": current,
        "stale": stale,
        "frozen": frozen,
        "by_target": by_target,
    })))
}

// ── Restore Operations ──

use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RestoreJob {
    pub job_id: String,
    pub target_name: String,
    pub source_node_id: String,
    pub destination_node_id: String,
    pub destination_path: Option<String>,
    pub path_prefix: Option<String>,
    pub mime_type_filter: Option<String>,
    pub status: String, // "running", "completed", "cancelled", "failed"
    pub files_scanned: u64,
    pub files_restored: u64,
    pub files_failed: u64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error: Option<String>,
}

/// In-memory restore job store (lives in AppState — added as an extension here).
/// For Phase 6, jobs are stored in memory only.
pub type RestoreJobStore = Arc<Mutex<HashMap<String, RestoreJob>>>;

#[derive(Deserialize)]
pub struct InitiateRestoreRequest {
    pub target_name: String,
    pub source_node_id: String,
    pub destination_node_id: String,
    pub destination_path: Option<String>,
    pub path_prefix: Option<String>,
    pub mime_type: Option<String>,
}

pub async fn initiate_restore(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InitiateRestoreRequest>,
) -> impl IntoResponse {
    if body.target_name.is_empty() || body.source_node_id.is_empty() || body.destination_node_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_json("validation", "target_name, source_node_id, destination_node_id are required")),
        );
    }

    // Verify storage backend exists
    let backend_id = format!("storage_backend::{}", body.target_name);
    let backend_doc = match state.db.get_document(&backend_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Storage backend not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let backend_type = backend_doc.get("backend").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let job_id = Uuid::new_v4().to_string();

    let job = RestoreJob {
        job_id: job_id.clone(),
        target_name: body.target_name.clone(),
        source_node_id: body.source_node_id.clone(),
        destination_node_id: body.destination_node_id.clone(),
        destination_path: body.destination_path.clone(),
        path_prefix: body.path_prefix.clone(),
        mime_type_filter: body.mime_type.clone(),
        status: "running".to_string(),
        files_scanned: 0,
        files_restored: 0,
        files_failed: 0,
        started_at: Utc::now().to_rfc3339(),
        completed_at: None,
        error: None,
    };

    state.restore_jobs.lock().unwrap().insert(job_id.clone(), job);

    // Spawn restore task
    let db = state.db.clone();
    let jobs = state.restore_jobs.clone();
    let target_name = body.target_name.clone();
    let source_node_id = body.source_node_id.clone();
    let destination_node_id = body.destination_node_id.clone();
    let destination_path = body.destination_path.clone();
    let path_prefix = body.path_prefix.clone();
    let mime_filter = body.mime_type.clone();
    let jid = job_id.clone();

    tokio::spawn(async move {
        run_restore_job(
            &db, &jobs, &jid,
            &backend_type, &target_name, &source_node_id, &destination_node_id,
            destination_path.as_deref(), path_prefix.as_deref(), mime_filter.as_deref(),
            backend_doc,
        ).await;
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({ "job_id": job_id })))
}

pub async fn get_restore_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let jobs = state.restore_jobs.lock().unwrap();
    match jobs.get(&job_id) {
        Some(job) => (StatusCode::OK, Json(serde_json::json!(job))),
        None => (StatusCode::NOT_FOUND, Json(error_json("not_found", "Restore job not found"))),
    }
}

pub async fn cancel_restore_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let mut jobs = state.restore_jobs.lock().unwrap();
    match jobs.get_mut(&job_id) {
        Some(job) => {
            if job.status == "running" {
                job.status = "cancelled".to_string();
                job.completed_at = Some(Utc::now().to_rfc3339());
                (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
            } else {
                (StatusCode::CONFLICT, Json(error_json("conflict", "Job is not running")))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(error_json("not_found", "Restore job not found"))),
    }
}

pub async fn list_restore_history(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let jobs = state.restore_jobs.lock().unwrap();
    let mut items: Vec<&RestoreJob> = jobs.values().collect();
    items.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    (StatusCode::OK, Json(serde_json::json!({ "items": items })))
}

/// Execute a restore job: find replica documents for files from source_node_id
/// and update their ownership to destination_node_id.
async fn run_restore_job(
    db: &crate::couchdb::CouchClient,
    jobs: &RestoreJobStore,
    job_id: &str,
    backend_type: &str,
    target_name: &str,
    source_node_id: &str,
    destination_node_id: &str,
    destination_path: Option<&str>,
    path_prefix: Option<&str>,
    mime_filter: Option<&str>,
    backend_doc: serde_json::Value,
) {
    let mut files_scanned = 0u64;
    let mut files_restored = 0u64;
    let mut files_failed = 0u64;

    // Find all replica documents for this target with source matching source_node_id
    let replicas_resp = db.all_docs_by_prefix(&format!("replica::"), true).await;
    let replicas = match replicas_resp {
        Ok(r) => r,
        Err(e) => {
            update_job_status(jobs, job_id, "failed", 0, 0, 0, Some(&e.to_string()));
            return;
        }
    };

    let mut batch: Vec<serde_json::Value> = Vec::new();

    for row in replicas.rows {
        let replica_doc = match row.doc {
            Some(d) => d,
            None => continue,
        };

        if replica_doc.get("type").and_then(|v| v.as_str()) != Some("replica") {
            continue;
        }
        if replica_doc.get("target_name").and_then(|v| v.as_str()) != Some(target_name) {
            continue;
        }
        let replica_source = replica_doc.get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str()).unwrap_or("");
        if replica_source != source_node_id {
            continue;
        }

        let file_id = match replica_doc.get("file_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        // Check cancellation
        if jobs.lock().unwrap().get(job_id).map(|j| j.status.as_str()) == Some("cancelled") {
            break;
        }

        files_scanned += 1;

        // Get the original file document
        let file_doc = match db.get_document(&file_id).await {
            Ok(d) => d,
            Err(_) => {
                files_failed += 1;
                continue;
            }
        };

        // Apply path_prefix filter
        if let Some(prefix) = path_prefix {
            let export_path = file_doc
                .get("source").and_then(|s| s.get("export_path")).and_then(|v| v.as_str())
                .unwrap_or("");
            if !export_path.starts_with(prefix) {
                continue;
            }
        }

        // Apply mime_type filter
        if let Some(mime) = mime_filter {
            let file_mime = file_doc.get("mime_type").and_then(|v| v.as_str()).unwrap_or("");
            if !file_mime.starts_with(mime) {
                continue;
            }
        }

        // Determine new export_path based on backend type
        let remote_key = replica_doc.get("remote_key").and_then(|v| v.as_str()).unwrap_or("");
        let new_export_path = match backend_type {
            "agent" => {
                // For agent targets, the replica already exists at a known path
                // The remote_key IS the path on the destination agent
                remote_key.to_string()
            }
            "directory" => {
                let dir_path = backend_doc.get("backend_config")
                    .and_then(|c| c.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
                format!("{}/{}", dir_path.trim_end_matches('/'), remote_key)
            }
            "s3" | "b2" => {
                // For S3/B2, files will be downloaded to destination_path
                match destination_path {
                    Some(dest) => {
                        // Extract filename from remote_key: {prefix}/{uuid8}/{filename}
                        let filename = remote_key.split('/').last().unwrap_or("restored_file");
                        format!("{}/{}", dest.trim_end_matches('/'), filename)
                    }
                    None => {
                        files_failed += 1;
                        continue;
                    }
                }
            }
            _ => {
                files_failed += 1;
                continue;
            }
        };

        // Update file document: change ownership to destination_node_id
        let mut updated_file = file_doc.clone();
        if let Some(source) = updated_file.get_mut("source") {
            source["node_id"] = serde_json::Value::String(destination_node_id.to_string());
            source["export_path"] = serde_json::Value::String(new_export_path.clone());
            source["export_parent"] = serde_json::Value::String(
                std::path::Path::new(&new_export_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
        }
        updated_file["migrated_from"] = serde_json::Value::String(source_node_id.to_string());

        batch.push(updated_file);

        if batch.len() >= 50 {
            match db.bulk_docs(&batch).await {
                Ok(results) => {
                    for r in &results {
                        if r.ok == Some(true) { files_restored += 1; } else { files_failed += 1; }
                    }
                }
                Err(_) => files_failed += batch.len() as u64,
            }
            batch.clear();
        }
    }

    // Flush remaining batch
    if !batch.is_empty() {
        match db.bulk_docs(&batch).await {
            Ok(results) => {
                for r in &results {
                    if r.ok == Some(true) { files_restored += 1; } else { files_failed += 1; }
                }
            }
            Err(_) => files_failed += batch.len() as u64,
        }
    }

    let final_status = if jobs.lock().unwrap().get(job_id).map(|j| j.status.as_str()) == Some("cancelled") {
        "cancelled"
    } else {
        "completed"
    };
    update_job_status(jobs, job_id, final_status, files_scanned, files_restored, files_failed, None);
}

fn update_job_status(
    jobs: &RestoreJobStore,
    job_id: &str,
    status: &str,
    files_scanned: u64,
    files_restored: u64,
    files_failed: u64,
    error: Option<&str>,
) {
    let mut store = jobs.lock().unwrap();
    if let Some(job) = store.get_mut(job_id) {
        job.status = status.to_string();
        job.files_scanned = files_scanned;
        job.files_restored = files_restored;
        job.files_failed = files_failed;
        job.completed_at = Some(Utc::now().to_rfc3339());
        job.error = error.map(|e| e.to_string());
    }
}
