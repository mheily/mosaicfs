use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};
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

/// Deterministic label assignment _id: keyed by file UUID
fn assignment_id(file_id: &str) -> String {
    // file_id may be "file::uuid" or just "uuid"
    let uuid_part = file_id.strip_prefix("file::").unwrap_or(file_id);
    format!("label_assignment::{}", uuid_part)
}

// ── List all labels ──

pub async fn list_labels(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Gather all unique labels from assignments and rules
    let mut all_labels: HashSet<String> = HashSet::new();

    // From assignments
    if let Ok(resp) = state.db.all_docs_by_prefix("label_assignment::", true).await {
        for row in &resp.rows {
            if let Some(doc) = &row.doc {
                if let Some(labels) = doc.get("labels").and_then(|v| v.as_array()) {
                    for label in labels {
                        if let Some(s) = label.as_str() {
                            all_labels.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }

    // From rules
    if let Ok(resp) = state.db.all_docs_by_prefix("label_rule::", true).await {
        for row in &resp.rows {
            if let Some(doc) = &row.doc {
                if let Some(labels) = doc.get("labels").and_then(|v| v.as_array()) {
                    for label in labels {
                        if let Some(s) = label.as_str() {
                            all_labels.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut labels: Vec<String> = all_labels.into_iter().collect();
    labels.sort();

    (StatusCode::OK, Json(serde_json::json!({
        "labels": labels,
        "total": labels.len(),
    })))
}

// ── Label assignments ──

#[derive(Deserialize, Default)]
pub struct AssignmentQuery {
    pub file_id: Option<String>,
}

pub async fn list_assignments(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AssignmentQuery>,
) -> impl IntoResponse {
    if let Some(ref file_id) = query.file_id {
        let doc_id = assignment_id(file_id);
        match state.db.get_document(&doc_id).await {
            Ok(mut doc) => {
                strip_internals(&mut doc);
                return (StatusCode::OK, Json(serde_json::json!({ "items": [doc], "total": 1 })));
            }
            Err(CouchError::NotFound(_)) => {
                return (StatusCode::OK, Json(serde_json::json!({ "items": [], "total": 0 })));
            }
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
            }
        }
    }

    let resp = match state.db.all_docs_by_prefix("label_assignment::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let items: Vec<serde_json::Value> = resp.rows.into_iter().filter_map(|row| {
        let mut doc = row.doc?;
        strip_internals(&mut doc);
        Some(doc)
    }).collect();
    let total = items.len();

    (StatusCode::OK, Json(serde_json::json!({ "items": items, "total": total })))
}

#[derive(Deserialize)]
pub struct UpsertAssignmentRequest {
    pub file_id: String,
    pub labels: Vec<String>,
}

pub async fn upsert_assignment(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpsertAssignmentRequest>,
) -> impl IntoResponse {
    let doc_id = assignment_id(&body.file_id);

    // Try to get existing for _rev (upsert semantics)
    let rev = state.db.get_document(&doc_id).await.ok()
        .and_then(|d| d.get("_rev").and_then(|v| v.as_str()).map(|s| s.to_string()));

    let file_uuid = body.file_id.strip_prefix("file::").unwrap_or(&body.file_id);

    let mut doc = serde_json::json!({
        "_id": doc_id,
        "type": "label_assignment",
        "file_id": format!("file::{}", file_uuid),
        "labels": body.labels,
        "updated_at": Utc::now().to_rfc3339(),
    });

    if let Some(rev) = rev {
        doc["_rev"] = serde_json::Value::String(rev);
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn delete_assignment(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AssignmentQuery>,
) -> impl IntoResponse {
    let file_id = match query.file_id {
        Some(ref id) => id,
        None => {
            return (StatusCode::BAD_REQUEST, Json(error_json("validation", "file_id query param required")));
        }
    };

    let doc_id = assignment_id(file_id);
    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Assignment not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let rev = doc.get("_rev").and_then(|v| v.as_str()).unwrap_or("");
    match state.db.delete_document(&doc_id, rev).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

// ── Label rules ──

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    pub labels: Vec<String>,
    pub node_id: Option<String>,
    pub path_prefix: Option<String>,
    pub glob: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

pub async fn list_rules(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("label_rule::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let items: Vec<serde_json::Value> = resp.rows.into_iter().filter_map(|row| {
        let mut doc = row.doc?;
        strip_internals(&mut doc);
        Some(doc)
    }).collect();
    let total = items.len();

    (StatusCode::OK, Json(serde_json::json!({ "items": items, "total": total })))
}

pub async fn create_rule(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRuleRequest>,
) -> impl IntoResponse {
    if body.labels.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(error_json("validation", "labels must not be empty")));
    }

    // Validate path_prefix has trailing /
    if let Some(ref prefix) = body.path_prefix {
        if !prefix.ends_with('/') {
            return (StatusCode::BAD_REQUEST, Json(error_json("validation", "path_prefix must end with /")));
        }
    }

    // Validate node_id exists if provided
    if let Some(ref node_id) = body.node_id {
        match state.db.get_document(&format!("node::{}", node_id)).await {
            Ok(_) => {}
            Err(CouchError::NotFound(_)) => {
                return (StatusCode::BAD_REQUEST, Json(error_json("validation", "node_id not found")));
            }
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
            }
        }
    }

    let rule_id = Uuid::new_v4().to_string()[..8].to_string();
    let doc_id = format!("label_rule::{}", rule_id);

    let doc = serde_json::json!({
        "_id": doc_id,
        "type": "label_rule",
        "rule_id": rule_id,
        "labels": body.labels,
        "node_id": body.node_id,
        "path_prefix": body.path_prefix,
        "glob": body.glob,
        "enabled": body.enabled,
        "created_at": Utc::now().to_rfc3339(),
    });

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => {
            let mut result = doc.clone();
            strip_internals(&mut result);
            (StatusCode::CREATED, Json(result))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn patch_rule(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let doc_id = format!("label_rule::{}", rule_id);
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Rule not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    // Validate path_prefix if being updated
    if let Some(prefix) = body.get("path_prefix").and_then(|v| v.as_str()) {
        if !prefix.ends_with('/') {
            return (StatusCode::BAD_REQUEST, Json(error_json("validation", "path_prefix must end with /")));
        }
    }

    // Apply updates
    if let Some(labels) = body.get("labels") {
        doc["labels"] = labels.clone();
    }
    if let Some(enabled) = body.get("enabled") {
        doc["enabled"] = enabled.clone();
    }
    if let Some(node_id) = body.get("node_id") {
        doc["node_id"] = node_id.clone();
    }
    if let Some(path_prefix) = body.get("path_prefix") {
        doc["path_prefix"] = path_prefix.clone();
    }
    if let Some(glob) = body.get("glob") {
        doc["glob"] = glob.clone();
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
) -> impl IntoResponse {
    let doc_id = format!("label_rule::{}", rule_id);
    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Rule not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let rev = doc.get("_rev").and_then(|v| v.as_str()).unwrap_or("");
    match state.db.delete_document(&doc_id, rev).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

// ── Effective labels ──

#[derive(Deserialize, Default)]
pub struct EffectiveQuery {
    pub file_id: Option<String>,
}

/// GET /api/labels/effective — returns the union of direct + rule-based labels for a file
pub async fn effective_labels(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EffectiveQuery>,
) -> impl IntoResponse {
    let file_id = match query.file_id {
        Some(ref id) => id.clone(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(error_json("validation", "file_id query param required")));
        }
    };

    let file_uuid = file_id.strip_prefix("file::").unwrap_or(&file_id);
    let full_file_id = format!("file::{}", file_uuid);

    // Get the file document to check against rules
    let file_doc = match state.db.get_document(&full_file_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "File not found")));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())));
        }
    };

    let mut labels: HashSet<String> = HashSet::new();

    // 1. Direct assignment labels
    let assign_id = assignment_id(&file_id);
    if let Ok(doc) = state.db.get_document(&assign_id).await {
        if let Some(label_arr) = doc.get("labels").and_then(|v| v.as_array()) {
            for l in label_arr {
                if let Some(s) = l.as_str() {
                    labels.insert(s.to_string());
                }
            }
        }
    }

    // 2. Rule-based labels
    let file_node_id = file_doc.get("source").and_then(|s| s.get("node_id")).and_then(|v| v.as_str()).unwrap_or("");
    let file_export_path = file_doc.get("source").and_then(|s| s.get("export_path")).and_then(|v| v.as_str()).unwrap_or("");
    let file_name = file_doc.get("name").and_then(|v| v.as_str()).unwrap_or("");

    if let Ok(resp) = state.db.all_docs_by_prefix("label_rule::", true).await {
        for row in resp.rows {
            if let Some(doc) = row.doc {
                if doc.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
                    continue;
                }

                // Check node_id match
                if let Some(rule_node) = doc.get("node_id").and_then(|v| v.as_str()) {
                    if rule_node != file_node_id {
                        continue;
                    }
                }

                // Check path_prefix match
                if let Some(prefix) = doc.get("path_prefix").and_then(|v| v.as_str()) {
                    if !file_export_path.starts_with(prefix) {
                        continue;
                    }
                }

                // Check glob match
                if let Some(glob_pattern) = doc.get("glob").and_then(|v| v.as_str()) {
                    if !simple_glob_match(file_name, glob_pattern) {
                        continue;
                    }
                }

                // Rule matches — add labels
                if let Some(rule_labels) = doc.get("labels").and_then(|v| v.as_array()) {
                    for l in rule_labels {
                        if let Some(s) = l.as_str() {
                            labels.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut labels: Vec<String> = labels.into_iter().collect();
    labels.sort();

    (StatusCode::OK, Json(serde_json::json!({
        "file_id": full_file_id,
        "labels": labels,
    })))
}

/// Simple glob match (reused from search module logic)
fn simple_glob_match(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();
    let (tlen, plen) = (text.len(), pattern.len());
    let mut dp = vec![vec![false; plen + 1]; tlen + 1];
    dp[0][0] = true;
    for j in 1..=plen {
        if pattern[j - 1] == b'*' { dp[0][j] = dp[0][j - 1]; }
    }
    for i in 1..=tlen {
        for j in 1..=plen {
            if pattern[j - 1] == b'*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if pattern[j - 1] == b'?' || pattern[j - 1] == text[i - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[tlen][plen]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assignment_id_deterministic() {
        assert_eq!(assignment_id("file::abc-123"), "label_assignment::abc-123");
        assert_eq!(assignment_id("abc-123"), "label_assignment::abc-123");
        // Same file always gets same assignment id
        assert_eq!(assignment_id("file::abc-123"), assignment_id("abc-123"));
    }

    #[test]
    fn test_simple_glob_match() {
        assert!(simple_glob_match("report.pdf", "*.pdf"));
        assert!(simple_glob_match("REPORT.PDF", "*.pdf"));
        assert!(!simple_glob_match("report.txt", "*.pdf"));
        assert!(simple_glob_match("test", "*"));
    }
}
