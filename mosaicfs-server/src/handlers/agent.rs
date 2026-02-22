use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, Method, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use tracing::warn;

use crate::auth::hmac_auth::HmacClaims;
use crate::couchdb::CouchError;
use crate::state::AppState;

/// Allowed document types for agent push replication (Flow 1)
const FLOW1_PUSH_TYPES: &[&str] = &[
    "file",
    "node",
    "agent_status",
    "utilization_snapshot",
    "annotation",
    "access",
    "replica",
    "notification",
];

/// Document types excluded from agent pull replication (Flow 2)
const FLOW2_PULL_EXCLUDE: &[&str] = &["agent_status", "utilization_snapshot"];

/// Document types excluded from browser replication (Flow 3)
const FLOW3_BROWSER_EXCLUDE: &[&str] = &["credential", "utilization_snapshot"];

/// Check if a document type is allowed for a given replication flow
pub fn is_allowed_flow1(doc_type: &str) -> bool {
    FLOW1_PUSH_TYPES.contains(&doc_type)
}

pub fn is_allowed_flow2(doc_type: &str) -> bool {
    !FLOW2_PULL_EXCLUDE.contains(&doc_type)
}

pub fn is_allowed_flow3(doc_type: &str) -> bool {
    !FLOW3_BROWSER_EXCLUDE.contains(&doc_type)
}

/// Proxy CouchDB replication requests from agents.
/// Agents authenticate with HMAC; the proxy forwards to CouchDB with admin credentials.
pub async fn replicate_proxy(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let method = req.method().clone();
    let uri_path = req.uri().path_and_query().map(|pq| pq.as_str().to_string()).unwrap_or_default();

    // Strip the /api/agent/replicate prefix and forward the rest to CouchDB
    let couch_path = uri_path
        .strip_prefix("/api/agent/replicate")
        .unwrap_or("/");

    let couch_url = format!("{}{}", state.couchdb_url, couch_path);

    let headers = req.headers().clone();
    let body_bytes = match axum::body::to_bytes(req.into_body(), 50 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Request body too large").into_response();
        }
    };

    // Filter documents in bulk_docs requests (Flow 1 push)
    let filtered_body = if couch_path.contains("_bulk_docs") && method == Method::POST {
        match filter_bulk_docs(&body_bytes) {
            Ok(filtered) => filtered,
            Err(e) => {
                warn!(error = %e, "Failed to filter bulk_docs");
                body_bytes.to_vec()
            }
        }
    } else {
        body_bytes.to_vec()
    };

    // Forward to CouchDB
    let client = Client::new();
    let mut builder = match method {
        Method::GET => client.get(&couch_url),
        Method::POST => client.post(&couch_url),
        Method::PUT => client.put(&couch_url),
        Method::DELETE => client.delete(&couch_url),
        Method::HEAD => client.head(&couch_url),
        _ => {
            return (StatusCode::METHOD_NOT_ALLOWED, "Method not allowed").into_response();
        }
    };

    builder = builder.basic_auth(&state.couchdb_user, Some(&state.couchdb_password));

    // Forward content-type header
    if let Some(ct) = headers.get(header::CONTENT_TYPE) {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }

    builder = builder.body(filtered_body);

    match builder.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body = resp.bytes().await.unwrap_or_default();
            (status, body).into_response()
        }
        Err(e) => {
            warn!(error = %e, "CouchDB proxy request failed");
            (StatusCode::BAD_GATEWAY, "CouchDB unreachable").into_response()
        }
    }
}

/// Filter a _bulk_docs payload to only include allowed document types (Flow 1)
fn filter_bulk_docs(body: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut parsed: serde_json::Value = serde_json::from_slice(body)?;
    if let Some(docs) = parsed.get_mut("docs").and_then(|v| v.as_array_mut()) {
        docs.retain(|doc| {
            let doc_type = doc.get("type").and_then(|v| v.as_str()).unwrap_or("");
            is_allowed_flow1(doc_type)
        });
    }
    Ok(serde_json::to_vec(&parsed)?)
}

// ── Agent internal endpoints ──

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

/// POST /api/agent/heartbeat — update node's last_heartbeat and status
pub async fn heartbeat(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let claims = req.extensions().get::<HmacClaims>().unwrap().clone();

    // Read body for optional node_id
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(error_json("bad_request", "Body too large"))),
    };

    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));
    let node_id = body.get("node_id").and_then(|v| v.as_str()).unwrap_or("");

    if node_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(error_json("validation", "node_id is required")));
    }

    let doc_id = format!("node::{}", node_id);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Node not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    doc["last_heartbeat"] = serde_json::Value::String(Utc::now().to_rfc3339());
    doc["status"] = serde_json::Value::String("online".to_string());

    // Merge optional fields from heartbeat body
    if let Some(storage) = body.get("storage") {
        doc["storage"] = storage.clone();
    }
    if let Some(version) = body.get("agent_version").and_then(|v| v.as_str()) {
        doc["agent_version"] = serde_json::Value::String(version.to_string());
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

/// POST /api/agent/files/bulk — bulk file upsert with partial failure handling
#[derive(Deserialize)]
pub struct BulkFilesRequest {
    pub docs: Vec<serde_json::Value>,
}

pub async fn bulk_files(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BulkFilesRequest>,
) -> impl IntoResponse {
    if body.docs.is_empty() {
        return (StatusCode::OK, Json(serde_json::json!({
            "results": [],
            "accepted": 0,
            "rejected": 0,
        })));
    }

    // Validate each doc is a file type and filter
    let mut valid_docs = Vec::new();
    let mut results: Vec<serde_json::Value> = Vec::new();

    for doc in &body.docs {
        let doc_type = doc.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let doc_id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("");

        if doc_type != "file" {
            results.push(serde_json::json!({
                "id": doc_id,
                "ok": false,
                "error": "invalid_type",
                "reason": "Only file documents are accepted",
            }));
            continue;
        }

        if doc_id.is_empty() {
            results.push(serde_json::json!({
                "id": null,
                "ok": false,
                "error": "missing_id",
                "reason": "_id is required",
            }));
            continue;
        }

        valid_docs.push(doc.clone());
    }

    // Bulk write valid docs
    if !valid_docs.is_empty() {
        match state.db.bulk_docs(&valid_docs).await {
            Ok(bulk_results) => {
                for br in bulk_results {
                    let id = br.id.unwrap_or_default();
                    if br.ok == Some(true) {
                        results.push(serde_json::json!({
                            "id": id,
                            "ok": true,
                            "rev": br.rev,
                        }));
                    } else {
                        results.push(serde_json::json!({
                            "id": id,
                            "ok": false,
                            "error": br.error.unwrap_or_else(|| "unknown".to_string()),
                            "reason": br.reason.unwrap_or_default(),
                        }));
                    }
                }
            }
            Err(e) => {
                // If the entire bulk operation fails, mark all valid docs as failed
                for doc in &valid_docs {
                    let doc_id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("");
                    results.push(serde_json::json!({
                        "id": doc_id,
                        "ok": false,
                        "error": "internal",
                        "reason": e.to_string(),
                    }));
                }
            }
        }
    }

    let accepted = results.iter().filter(|r| r.get("ok") == Some(&serde_json::Value::Bool(true))).count();
    let rejected = results.len() - accepted;

    (StatusCode::OK, Json(serde_json::json!({
        "results": results,
        "accepted": accepted,
        "rejected": rejected,
    })))
}

/// POST /api/agent/status — agent status report
pub async fn agent_status(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let node_id = match body.get("node_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return (StatusCode::BAD_REQUEST, Json(error_json("validation", "node_id is required")));
        }
    };

    let doc_id = format!("status::{}", node_id);

    // Try to get existing for _rev
    let rev = state.db.get_document(&doc_id).await.ok()
        .and_then(|d| d.get("_rev").and_then(|v| v.as_str()).map(|s| s.to_string()));

    let mut doc = body.clone();
    doc["_id"] = serde_json::Value::String(doc_id.clone());
    doc["type"] = serde_json::Value::String("agent_status".to_string());
    doc["updated_at"] = serde_json::Value::String(Utc::now().to_rfc3339());
    if let Some(rev) = rev {
        doc["_rev"] = serde_json::Value::String(rev);
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

/// POST /api/agent/utilization — utilization snapshot
pub async fn agent_utilization(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let node_id = match body.get("node_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return (StatusCode::BAD_REQUEST, Json(error_json("validation", "node_id is required")));
        }
    };

    let doc_id = format!("utilization::{}::{}", node_id, Utc::now().format("%Y%m%dT%H%M%S"));

    let mut doc = body.clone();
    doc["_id"] = serde_json::Value::String(doc_id.clone());
    doc["type"] = serde_json::Value::String("utilization_snapshot".to_string());
    doc["timestamp"] = serde_json::Value::String(Utc::now().to_rfc3339());

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

/// GET /api/agent/credentials — return HMAC key for the authenticated agent
pub async fn agent_credentials(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let claims = req.extensions().get::<HmacClaims>().unwrap().clone();

    match state.db.get_document(&format!("credential::{}", claims.access_key_id)).await {
        Ok(doc) => {
            // Return only safe fields
            (StatusCode::OK, Json(serde_json::json!({
                "access_key_id": claims.access_key_id,
                "name": doc.get("name"),
                "enabled": doc.get("enabled"),
            })))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

/// GET /api/agent/transfer/{file_id} — proxy file content from the agent's local filesystem
/// For Phase 2, this returns the file metadata with source info so the caller can fetch directly.
pub async fn agent_transfer(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    let doc_id = if file_id.starts_with("file::") {
        file_id.clone()
    } else {
        format!("file::{}", file_id)
    };

    match state.db.get_document(&doc_id).await {
        Ok(doc) => {
            // Record access
            state.record_access(&doc_id);

            let source = doc.get("source").cloned().unwrap_or(serde_json::json!({}));
            let node_id = source.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
            let export_path = source.get("export_path").and_then(|v| v.as_str()).unwrap_or("");

            (StatusCode::OK, Json(serde_json::json!({
                "file_id": doc_id,
                "node_id": node_id,
                "export_path": export_path,
                "size": doc.get("size"),
                "mime_type": doc.get("mime_type"),
            })))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "File not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow1_filters() {
        assert!(is_allowed_flow1("file"));
        assert!(is_allowed_flow1("node"));
        assert!(is_allowed_flow1("notification"));
        assert!(!is_allowed_flow1("credential"));
        assert!(!is_allowed_flow1("virtual_directory"));
        assert!(!is_allowed_flow1("label_rule"));
    }

    #[test]
    fn test_flow2_filters() {
        assert!(is_allowed_flow2("file"));
        assert!(is_allowed_flow2("credential"));
        assert!(is_allowed_flow2("virtual_directory"));
        assert!(!is_allowed_flow2("agent_status"));
        assert!(!is_allowed_flow2("utilization_snapshot"));
    }

    #[test]
    fn test_flow3_filters() {
        assert!(is_allowed_flow3("file"));
        assert!(is_allowed_flow3("node"));
        assert!(!is_allowed_flow3("credential"));
        assert!(!is_allowed_flow3("utilization_snapshot"));
    }

    #[test]
    fn test_filter_bulk_docs() {
        let input = serde_json::json!({
            "docs": [
                {"_id": "file::1", "type": "file", "name": "test.txt"},
                {"_id": "credential::1", "type": "credential", "name": "bad"},
                {"_id": "node::1", "type": "node", "friendly_name": "laptop"},
            ]
        });
        let result = filter_bulk_docs(&serde_json::to_vec(&input).unwrap()).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
        let docs = parsed["docs"].as_array().unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0]["type"], "file");
        assert_eq!(docs[1]["type"], "node");
    }
}
