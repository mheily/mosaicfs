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

#[derive(Deserialize, Default)]
pub struct ListNodesQuery {
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn list_nodes(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListNodesQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("node::", true).await {
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
            if doc.get("type")?.as_str()? != "node" {
                return None;
            }
            if let Some(ref status_filter) = query.status {
                if doc.get("status")?.as_str()? != status_filter {
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

pub async fn get_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_document(&format!("node::{}", node_id)).await {
        Ok(mut doc) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "Node not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

#[derive(Deserialize)]
pub struct RegisterNodeRequest {
    pub friendly_name: Option<String>,
    pub platform: Option<String>,
}

pub async fn register_node(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterNodeRequest>,
) -> impl IntoResponse {
    let node_id = format!("node-{}", &Uuid::new_v4().to_string()[..8]);
    let doc = serde_json::json!({
        "_id": format!("node::{}", node_id),
        "type": "node",
        "friendly_name": body.friendly_name.unwrap_or_else(|| node_id.clone()),
        "platform": body.platform.unwrap_or_else(|| "unknown".to_string()),
        "status": "online",
        "last_heartbeat": Utc::now().to_rfc3339(),
        "vfs_capable": false,
        "capabilities": [],
    });

    match state.db.put_document(&format!("node::{}", node_id), &doc).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({ "node_id": node_id }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn patch_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
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

    if let Some(name) = body.get("friendly_name").and_then(|v| v.as_str()) {
        doc["friendly_name"] = serde_json::Value::String(name.to_string());
    }
    if let Some(watch_paths) = body.get("watch_paths") {
        doc["watch_paths"] = watch_paths.clone();
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn delete_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> impl IntoResponse {
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

    // Soft disable
    doc["status"] = serde_json::Value::String("disabled".to_string());

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn get_node_status(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_document(&format!("status::{}", node_id)).await {
        Ok(mut doc) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "Agent status not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

// ── Network Mounts CRUD ──

pub async fn list_mounts(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_document(&format!("node::{}", node_id)).await {
        Ok(doc) => {
            let mounts = doc.get("network_mounts").cloned().unwrap_or(serde_json::json!([]));
            (StatusCode::OK, Json(serde_json::json!({ "items": mounts })))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("not_found", "Node not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string())))
        }
    }
}

#[derive(Deserialize)]
pub struct AddMountRequest {
    pub remote_node_id: String,
    pub remote_base_export_path: String,
    pub local_mount_path: String,
    pub mount_type: String,
    #[serde(default)]
    pub priority: i32,
}

pub async fn add_mount(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Json(body): Json<AddMountRequest>,
) -> impl IntoResponse {
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

    let mount_id = Uuid::new_v4().to_string()[..8].to_string();
    let mount = serde_json::json!({
        "mount_id": mount_id,
        "remote_node_id": body.remote_node_id,
        "remote_base_export_path": body.remote_base_export_path,
        "local_mount_path": body.local_mount_path,
        "mount_type": body.mount_type,
        "priority": body.priority,
    });

    let mounts = doc
        .as_object_mut()
        .unwrap()
        .entry("network_mounts")
        .or_insert_with(|| serde_json::json!([]));
    mounts.as_array_mut().unwrap().push(mount);

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({ "mount_id": mount_id }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn patch_mount(
    State(state): State<Arc<AppState>>,
    Path((node_id, mount_id)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
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

    let mounts = match doc.get_mut("network_mounts").and_then(|v| v.as_array_mut()) {
        Some(m) => m,
        None => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Mount not found")));
        }
    };

    let mount = match mounts.iter_mut().find(|m| m.get("mount_id").and_then(|v| v.as_str()) == Some(&mount_id)) {
        Some(m) => m,
        None => {
            return (StatusCode::NOT_FOUND, Json(error_json("not_found", "Mount not found")));
        }
    };

    // Apply updates
    if let Some(obj) = body.as_object() {
        for (k, v) in obj {
            if k != "mount_id" {
                mount[k] = v.clone();
            }
        }
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

pub async fn delete_mount(
    State(state): State<Arc<AppState>>,
    Path((node_id, mount_id)): Path<(String, String)>,
) -> impl IntoResponse {
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

    if let Some(mounts) = doc.get_mut("network_mounts").and_then(|v| v.as_array_mut()) {
        mounts.retain(|m| m.get("mount_id").and_then(|v| v.as_str()) != Some(&mount_id));
    }

    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("internal", &e.to_string()))),
    }
}

fn strip_internals(doc: &mut serde_json::Value) {
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("_rev");
        // Keep _id but rename to id for API convention
        if let Some(id) = obj.remove("_id") {
            obj.insert("id".to_string(), id);
        }
    }
}

fn error_json(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "code": code, "message": message } })
}
