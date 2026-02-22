use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tracing::warn;

use crate::couchdb::CouchError;
use crate::state::AppState;

#[derive(Deserialize, Default)]
pub struct ListFilesQuery {
    pub node_id: Option<String>,
    pub status: Option<String>,
    pub mime_type: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn list_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListFilesQuery>,
) -> impl IntoResponse {
    let resp = match state.db.all_docs_by_prefix("file::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string())));
        }
    };

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            if doc.get("type")?.as_str()? != "file" {
                return None;
            }
            if let Some(ref node_id) = query.node_id {
                let doc_node = doc.get("source")?.get("node_id")?.as_str()?;
                if doc_node != node_id {
                    return None;
                }
            }
            if let Some(ref status) = query.status {
                if doc.get("status")?.as_str()? != status {
                    return None;
                }
            } else {
                // Default: active only
                if doc.get("status").and_then(|v| v.as_str()) != Some("active") {
                    return None;
                }
            }
            if let Some(ref mime) = query.mime_type {
                let doc_mime = doc.get("mime_type").and_then(|v| v.as_str()).unwrap_or("");
                if doc_mime != mime {
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

pub async fn get_file(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    // Support both "file::uuid" and just "uuid"
    let doc_id = if file_id.starts_with("file::") {
        file_id.clone()
    } else {
        format!("file::{}", file_id)
    };

    match state.db.get_document(&doc_id).await {
        Ok(mut doc) => {
            strip_internals(&mut doc);
            (StatusCode::OK, Json(doc))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, Json(error_json("File not found")))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string())))
        }
    }
}

#[derive(Deserialize)]
pub struct ByPathQuery {
    pub path: String,
}

pub async fn get_file_by_path(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ByPathQuery>,
) -> impl IntoResponse {
    // Search for file by export_path
    let selector = serde_json::json!({
        "type": "file",
        "source.export_path": query.path,
        "status": "active",
    });

    match state.db.find(selector).await {
        Ok(resp) => {
            if let Some(mut doc) = resp.docs.into_iter().next() {
                strip_internals(&mut doc);
                (StatusCode::OK, Json(doc))
            } else {
                (StatusCode::NOT_FOUND, Json(error_json("File not found at path")))
            }
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string())))
        }
    }
}

/// GET /api/files/{file_id}/content — serve file content with Range support
pub async fn get_file_content(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let doc_id = if file_id.starts_with("file::") {
        file_id.clone()
    } else {
        format!("file::{}", file_id)
    };

    let doc = match state.db.get_document(&doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("File not found"))).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string()))).into_response();
        }
    };

    // Record access
    state.record_access(&doc_id);

    let name = doc.get("name").and_then(|v| v.as_str()).unwrap_or("file");
    let mime_type = doc.get("mime_type").and_then(|v| v.as_str()).unwrap_or("application/octet-stream");
    let file_size = doc.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

    // Get the source path — for local files, read directly
    let export_path = doc
        .get("source")
        .and_then(|s| s.get("export_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if export_path.is_empty() {
        return (StatusCode::NOT_FOUND, Json(error_json("File has no source path"))).into_response();
    }

    // Try local file first
    let path = std::path::Path::new(export_path);
    if !path.exists() {
        // TODO: Phase 2 Tier 4 — fetch from remote agent via HTTP
        // For now, return 404 if the file is not locally accessible
        return (StatusCode::NOT_FOUND, Json(error_json("File not accessible on this server"))).into_response();
    }

    let metadata = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, path = export_path, "Failed to stat file");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("Failed to read file"))).into_response();
        }
    };
    let total_size = metadata.len();

    // Parse Range header
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(parse_range);

    let disposition = format!("attachment; filename=\"{}\"", name.replace('"', "\\\""));

    match range {
        Some((start, end)) => {
            let end = end.unwrap_or(total_size - 1).min(total_size - 1);
            if start > end || start >= total_size {
                return (StatusCode::RANGE_NOT_SATISFIABLE, "Invalid range").into_response();
            }
            let len = end - start + 1;

            let mut file = match tokio::fs::File::open(path).await {
                Ok(f) => f,
                Err(e) => {
                    warn!(error = %e, "Failed to open file");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
                }
            };

            use tokio::io::AsyncSeekExt;
            if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
                warn!(error = %e, "Failed to seek");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to seek").into_response();
            }

            let mut buf = vec![0u8; len as usize];
            if let Err(e) = file.read_exact(&mut buf).await {
                warn!(error = %e, "Failed to read");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file").into_response();
            }

            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, mime_type)
                .header(header::CONTENT_LENGTH, len.to_string())
                .header(header::CONTENT_DISPOSITION, &disposition)
                .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, total_size))
                .header(header::ACCEPT_RANGES, "bytes")
                .body(Body::from(buf))
                .unwrap()
        }
        None => {
            // Full file — include Digest header (SHA-256)
            let mut file = match tokio::fs::File::open(path).await {
                Ok(f) => f,
                Err(e) => {
                    warn!(error = %e, "Failed to open file");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
                }
            };

            let mut buf = Vec::with_capacity(total_size as usize);
            if let Err(e) = file.read_to_end(&mut buf).await {
                warn!(error = %e, "Failed to read file");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file").into_response();
            }

            let digest = Sha256::digest(&buf);
            let digest_header = format!("sha-256=:{}", base64_encode(&digest));

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type)
                .header(header::CONTENT_LENGTH, buf.len().to_string())
                .header(header::CONTENT_DISPOSITION, &disposition)
                .header(header::ACCEPT_RANGES, "bytes")
                .header("Digest", &digest_header)
                .body(Body::from(buf))
                .unwrap()
        }
    }
}

/// GET /api/files/{file_id}/access — get access record for a file
pub async fn get_file_access(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    let file_uuid = file_id.strip_prefix("file::").unwrap_or(&file_id);
    let doc_id = format!("access::{}", file_uuid);

    match state.db.get_document(&doc_id).await {
        Ok(doc) => {
            (StatusCode::OK, Json(serde_json::json!({
                "file_id": format!("file::{}", file_uuid),
                "last_access": doc.get("last_access"),
            })))
        }
        Err(CouchError::NotFound(_)) => {
            (StatusCode::OK, Json(serde_json::json!({
                "file_id": format!("file::{}", file_uuid),
                "last_access": null,
            })))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string())))
        }
    }
}

/// Parse a Range header value like "bytes=0-499" or "bytes=500-"
fn parse_range(range_str: &str) -> Option<(u64, Option<u64>)> {
    let range_str = range_str.strip_prefix("bytes=")?;
    let mut parts = range_str.splitn(2, '-');
    let start: u64 = parts.next()?.parse().ok()?;
    let end: Option<u64> = parts.next().and_then(|s| {
        if s.is_empty() {
            None
        } else {
            s.parse().ok()
        }
    });
    Some((start, end))
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn strip_internals(doc: &mut serde_json::Value) {
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("_rev");
        if let Some(id) = obj.remove("_id") {
            obj.insert("id".to_string(), id);
        }
    }
}

fn error_json(message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "code": "error", "message": message } })
}
