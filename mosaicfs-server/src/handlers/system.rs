use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::couchdb;
use crate::state::AppState;

fn error_json(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "code": code, "message": message } })
}

// ── GET /api/health ──

pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.db_info().await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ok", "couchdb": "ok" })),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "degraded", "couchdb": "unreachable" })),
        ),
    }
}

// ── GET /api/system/info ──

pub async fn system_info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let elapsed = state.started_at.elapsed();
    let hours = elapsed.as_secs() / 3600;
    let minutes = (elapsed.as_secs() % 3600) / 60;
    let uptime = format!("{}h {}m", hours, minutes);

    let (doc_count, update_seq) = match state.db.db_info().await {
        Ok(info) => {
            let count = info.get("doc_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let seq = info
                .get("update_seq")
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_else(|| "0".to_string());
            (count, seq)
        }
        Err(_) => (0, "0".to_string()),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptime": uptime,
            "pouchdb_doc_count": doc_count,
            "pouchdb_update_seq": update_seq,
            "developer_mode": state.developer_mode,
        })),
    )
}

// ── GET /api/system/backup?type=minimal|full ──

#[derive(Deserialize)]
pub struct BackupQuery {
    #[serde(rename = "type", default = "default_backup_type")]
    pub backup_type: String,
}

fn default_backup_type() -> String {
    "minimal".to_string()
}

/// Minimal backup includes only user-generated configuration types.
const MINIMAL_TYPES: &[&str] = &[
    "virtual_directory",
    "label_assignment",
    "label_rule",
    "annotation",
    "credential",
    "plugin",
    "storage_backend",
    "replication_rule",
    "node",
];

/// For node documents in minimal backup, keep only these fields.
const NODE_PARTIAL_FIELDS: &[&str] = &["_id", "type", "friendly_name", "network_mounts"];

/// Redact plugin secrets based on settings_schema.
fn redact_plugin_secrets(doc: &mut serde_json::Value) {
    let schema = doc.get("settings_schema").cloned();
    if let Some(serde_json::Value::Object(schema_map)) = schema {
        if let Some(serde_json::Value::Object(settings)) = doc.get_mut("settings") {
            for (key, field_def) in &schema_map {
                if field_def.get("type").and_then(|v| v.as_str()) == Some("secret") {
                    if settings.contains_key(key) {
                        settings.insert(key.clone(), serde_json::json!("__REDACTED__"));
                    }
                }
            }
        }
    }
}

pub async fn backup(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BackupQuery>,
) -> impl IntoResponse {
    if query.backup_type != "minimal" && query.backup_type != "full" {
        return (
            StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            Json(error_json("bad_request", "type must be 'minimal' or 'full'")),
        )
            .into_response();
    }

    let all_docs = match state.db.all_docs(true).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            )
                .into_response();
        }
    };

    let is_minimal = query.backup_type == "minimal";

    let mut docs: Vec<serde_json::Value> = Vec::new();

    for row in all_docs.rows {
        // Skip design documents
        if row.id.starts_with("_design/") {
            continue;
        }

        let Some(mut doc) = row.doc else { continue };

        let doc_type = doc.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();

        if is_minimal && !MINIMAL_TYPES.contains(&doc_type.as_str()) {
            continue;
        }

        // Strip _rev (instance-specific)
        if let Some(obj) = doc.as_object_mut() {
            obj.remove("_rev");
            obj.remove("_conflicts");
        }

        // Redact plugin secrets (both minimal and full)
        if doc_type == "plugin" {
            redact_plugin_secrets(&mut doc);
        }

        // For minimal backup, node documents are partial
        if is_minimal && doc_type == "node" {
            let mut partial = serde_json::Map::new();
            if let Some(obj) = doc.as_object() {
                for &field in NODE_PARTIAL_FIELDS {
                    if let Some(val) = obj.get(field) {
                        partial.insert(field.to_string(), val.clone());
                    }
                }
            }
            docs.push(serde_json::Value::Object(partial));
        } else {
            docs.push(doc);
        }
    }

    let body = serde_json::json!({ "docs": docs });

    // Build filename with timestamp
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y-%m-%dT%H-%M-%SZ");
    let filename = format!("mosaicfs-backup-{}-{}.json", query.backup_type, timestamp);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", filename)
            .parse()
            .unwrap(),
    );

    (StatusCode::OK, headers, Json(body)).into_response()
}

// ── GET /api/system/backup/status ──

pub async fn backup_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let count = match count_non_design_docs(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "empty": count == 0,
            "document_count": count,
        })),
    )
}

async fn count_non_design_docs(state: &AppState) -> Result<u64, couchdb::CouchError> {
    let resp = state.db.all_docs(false).await?;
    let count = resp
        .rows
        .iter()
        .filter(|r| !r.id.starts_with("_design/"))
        .count() as u64;
    Ok(count)
}

// ── POST /api/system/restore ──

const RECOGNIZED_TYPES: &[&str] = &[
    "file",
    "node",
    "virtual_directory",
    "credential",
    "label_assignment",
    "label_rule",
    "plugin",
    "annotation",
    "storage_backend",
    "replication_rule",
    "replica",
    "access",
    "agent_status",
    "utilization_snapshot",
    "notification",
];

const BULK_BATCH_SIZE: usize = 500;

pub async fn restore(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // 1. Validate format
    let docs_array = match body.get("docs").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_json("bad_request", "Body must have a 'docs' array")),
            );
        }
    };

    // 2. Check empty database
    let doc_count = match count_non_design_docs(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    };

    if doc_count > 0 {
        return (
            StatusCode::CONFLICT,
            Json(error_json(
                "database_not_empty",
                "Restore only permitted into an empty database. Use developer mode wipe or recreate the stack.",
            )),
        );
    }

    // 3. Validate document types
    for doc in docs_array {
        let doc_type = doc.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if doc_type.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_json("bad_request", "Document missing 'type' field")),
            );
        }
        if !RECOGNIZED_TYPES.contains(&doc_type) {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_json(
                    "bad_request",
                    &format!("Unrecognized document type: '{}'", doc_type),
                )),
            );
        }
    }

    // 4. Prepare documents: strip _rev and _conflicts
    let mut prepared: Vec<serde_json::Value> = Vec::with_capacity(docs_array.len());
    for doc in docs_array {
        let mut doc = doc.clone();
        if let Some(obj) = doc.as_object_mut() {
            obj.remove("_rev");
            obj.remove("_conflicts");
        }
        prepared.push(doc);
    }

    // 5. Bulk write in batches
    let mut restored_count: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for batch in prepared.chunks(BULK_BATCH_SIZE) {
        match state.db.bulk_docs(batch).await {
            Ok(results) => {
                for result in &results {
                    if result.ok == Some(true) {
                        restored_count += 1;
                    } else {
                        let id = result.id.as_deref().unwrap_or("unknown");
                        let reason = result
                            .reason
                            .as_deref()
                            .or(result.error.as_deref())
                            .unwrap_or("unknown error");
                        errors.push(format!("{}: {}", id, reason));
                    }
                }
            }
            Err(e) => {
                errors.push(format!("Batch write failed: {}", e));
            }
        }
    }

    // 6. Rebuild caches
    if let Err(e) = state.label_cache.build(&state.db).await {
        tracing::error!(error = %e, "Failed to rebuild label cache after restore");
        errors.push(format!("Label cache rebuild failed: {}", e));
    }
    if let Err(e) = state.access_cache.build(&state.db).await {
        tracing::error!(error = %e, "Failed to rebuild access cache after restore");
        errors.push(format!("Access cache rebuild failed: {}", e));
    }

    // 7. Recreate indexes
    if let Err(e) = couchdb::create_indexes(&state.db).await {
        tracing::error!(error = %e, "Failed to recreate indexes after restore");
        errors.push(format!("Index creation failed: {}", e));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": errors.is_empty(),
            "restored_count": restored_count,
            "errors": errors,
        })),
    )
}

// ── DELETE /api/system/data ──

pub async fn delete_data(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // 1. Check developer mode
    if !state.developer_mode {
        return (
            StatusCode::FORBIDDEN,
            Json(error_json("forbidden", "Developer mode is not enabled")),
        );
    }

    // 2. Validate confirmation token
    let confirm = body.get("confirm").and_then(|v| v.as_str()).unwrap_or("");
    if confirm != "DELETE_ALL_DATA" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_json(
                "bad_request",
                "Body must contain {\"confirm\": \"DELETE_ALL_DATA\"}",
            )),
        );
    }

    // 3. Delete and recreate database
    if let Err(e) = state.db.delete_db().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json("internal", &format!("Failed to delete database: {}", e))),
        );
    }

    if let Err(e) = state.db.ensure_db().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json("internal", &format!("Failed to recreate database: {}", e))),
        );
    }

    if let Err(e) = couchdb::create_indexes(&state.db).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json("internal", &format!("Failed to recreate indexes: {}", e))),
        );
    }

    // 4. Clear in-memory caches by rebuilding from empty DB
    let _ = state.label_cache.build(&state.db).await;
    let _ = state.access_cache.build(&state.db).await;

    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}
