use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;

use crate::couchdb::CouchError;
use crate::state::AppState;

#[derive(Deserialize, Default)]
pub struct ListNotificationsQuery {
    pub status: Option<String>,
    pub severity: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn list_notifications(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListNotificationsQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("notification::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    };

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            if doc.get("type")?.as_str()? != "notification" {
                return None;
            }
            // Default: show active + unacknowledged (not resolved)
            let doc_status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("active");
            if let Some(ref status_filter) = query.status {
                if doc_status != status_filter {
                    return None;
                }
            } else if doc_status == "resolved" {
                return None;
            }
            if let Some(ref severity_filter) = query.severity {
                if doc.get("severity").and_then(|v| v.as_str()).unwrap_or("") != severity_filter {
                    return None;
                }
            }
            strip_internals(&mut doc);
            Some(doc)
        })
        .collect();

    // Sort by severity (error > warning > info) then last_seen descending
    items.sort_by(|a, b| {
        let sev_ord = |s: &serde_json::Value| -> u8 {
            match s.get("severity").and_then(|v| v.as_str()).unwrap_or("info") {
                "error" => 0,
                "warning" => 1,
                _ => 2,
            }
        };
        let cmp = sev_ord(a).cmp(&sev_ord(b));
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }
        let ts = |s: &serde_json::Value| -> String {
            s.get("last_seen")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        ts(b).cmp(&ts(a))
    });

    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(100) as usize;
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit,
        })),
    )
}

pub async fn acknowledge_notification(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // The id may already include the "notification::" prefix (from strip_internals renaming _id to id)
    let doc_id = if id.starts_with("notification::") {
        id.clone()
    } else {
        format!("notification::{}", id)
    };
    let mut doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(error_json("not_found", "Notification not found")),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    };

    doc["status"] = serde_json::Value::String("acknowledged".to_string());
    doc["acknowledged_at"] = serde_json::Value::String(Utc::now().to_rfc3339());

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json("internal", &e.to_string())),
        ),
    }
}

#[derive(Deserialize, Default)]
pub struct AcknowledgeAllQuery {
    pub severity: Option<String>,
}

pub async fn acknowledge_all(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AcknowledgeAllQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("notification::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    };

    let now = Utc::now().to_rfc3339();
    let mut docs_to_update: Vec<serde_json::Value> = Vec::new();

    for row in resp.rows {
        if let Some(mut doc) = row.doc {
            let doc_type = doc.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if doc_type != "notification" || status != "active" {
                continue;
            }
            if let Some(ref sev) = query.severity {
                if doc.get("severity").and_then(|v| v.as_str()).unwrap_or("") != sev {
                    continue;
                }
            }
            doc["status"] = serde_json::Value::String("acknowledged".to_string());
            doc["acknowledged_at"] = serde_json::Value::String(now.clone());
            docs_to_update.push(doc);
        }
    }

    let count = docs_to_update.len();
    if !docs_to_update.is_empty() {
        if let Err(e) = state.db.bulk_docs(&docs_to_update).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "count": count })),
    )
}

#[derive(Deserialize, Default)]
pub struct NotificationHistoryQuery {
    pub limit: Option<u64>,
    pub since: Option<String>,
}

pub async fn notification_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NotificationHistoryQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("notification::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json("internal", &e.to_string())),
            );
        }
    };

    let since = query.since.as_deref().unwrap_or("");

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            if doc.get("type")?.as_str()? != "notification" {
                return None;
            }
            let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if status != "resolved" && status != "acknowledged" {
                return None;
            }
            // Filter by since timestamp
            if !since.is_empty() {
                let ts = doc
                    .get("resolved_at")
                    .or_else(|| doc.get("acknowledged_at"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if ts < since {
                    return None;
                }
            }
            strip_internals(&mut doc);
            Some(doc)
        })
        .collect();

    // Sort by timestamp descending
    items.sort_by(|a, b| {
        let ts = |d: &serde_json::Value| -> String {
            d.get("resolved_at")
                .or_else(|| d.get("acknowledged_at"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        ts(b).cmp(&ts(a))
    });

    let limit = query.limit.unwrap_or(50) as usize;
    let items: Vec<_> = items.into_iter().take(limit).collect();

    (StatusCode::OK, Json(serde_json::json!({ "items": items })))
}

fn strip_internals(doc: &mut serde_json::Value) {
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("_rev");
        if let Some(id) = obj.remove("_id") {
            obj.insert("id".to_string(), id);
        }
    }
}

fn error_json(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "code": code, "message": message } })
}
