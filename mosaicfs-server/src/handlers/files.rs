use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;
use tracing::warn;

use crate::couchdb::CouchError;
use crate::state::AppState;

// ── Download tokens ──────────────────────────────────────────────────────────
//
// A download token is a short-lived credential that allows unauthenticated
// access to a specific file's content. It is HMAC-SHA256 signed with the
// server's JWT secret so no server-side storage is required.
//
// Token format: "{expiry_unix_ts}.{hmac_hex}"
// HMAC message: "dl:{doc_id}:{expiry_unix_ts}"

const DOWNLOAD_TOKEN_EXPIRY_SECS: i64 = 7200; // 2 hours — enough for video streaming

fn issue_download_token(secret: &[u8], doc_id: &str) -> String {
    let expiry = Utc::now().timestamp() + DOWNLOAD_TOKEN_EXPIRY_SECS;
    let message = format!("dl:{}:{}", doc_id, expiry);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(message.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{}.{}", expiry, sig)
}

fn validate_download_token(secret: &[u8], doc_id: &str, token: &str) -> bool {
    let (expiry_str, sig) = match token.split_once('.') {
        Some(pair) => pair,
        None => return false,
    };
    let expiry: i64 = match expiry_str.parse() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if Utc::now().timestamp() > expiry {
        return false;
    }
    let message = format!("dl:{}:{}", doc_id, expiry_str);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(message.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    constant_time_eq(sig.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

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

/// POST /api/files/{file_id}/token — issue a short-lived download token
pub async fn get_file_token(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    let doc_id = if file_id.starts_with("file::") {
        file_id.clone()
    } else {
        format!("file::{}", file_id)
    };

    // Verify the file exists before issuing a token
    match state.db.get_document(&doc_id).await {
        Ok(_) => {}
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("File not found"))).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string()))).into_response();
        }
    }

    let token = issue_download_token(&state.jwt_secret, &doc_id);
    let encoded_id = urlencoding::encode(&doc_id).into_owned();
    let encoded_token = urlencoding::encode(&token).into_owned();
    let url = format!("/api/files/{}/download?token={}", encoded_id, encoded_token);

    (StatusCode::OK, Json(serde_json::json!({
        "token": token,
        "url": url,
        "expires_in": DOWNLOAD_TOKEN_EXPIRY_SECS,
    }))).into_response()
}

/// GET /api/files/{file_id}/download?token=… — token-authenticated content download
/// (public route; JWT not required — the signed token provides authorization)
pub async fn download_file(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
    Query(query): Query<DownloadTokenQuery>,
    headers: HeaderMap,
) -> Response {
    let doc_id = if file_id.starts_with("file::") {
        file_id.clone()
    } else {
        format!("file::{}", file_id)
    };

    if !validate_download_token(&state.jwt_secret, &doc_id, &query.token) {
        return (StatusCode::UNAUTHORIZED, "Invalid or expired download token").into_response();
    }

    serve_file_content(&state, &doc_id, headers).await
}

#[derive(Deserialize)]
pub struct DownloadTokenQuery {
    pub token: String,
}

/// Shared file-serving logic used by both the JWT-authed and token-authed routes.
async fn serve_file_content(state: &AppState, doc_id: &str, headers: HeaderMap) -> Response {
    let doc = match state.db.get_document(doc_id).await {
        Ok(d) => d,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, Json(error_json("File not found"))).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json(&e.to_string()))).into_response();
        }
    };

    // Record access
    state.record_access(doc_id);

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

    // Parse Range header early — needed for both local and proxy paths
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(parse_range);
    let disposition = format!("inline; filename=\"{}\"", name.replace('"', "\\\""));

    // Try local file first; if not present, proxy to the source agent
    let path = std::path::Path::new(export_path);
    if !path.exists() {
        return proxy_to_agent(&state, &doc, export_path, file_size, range, mime_type, &disposition).await;
    }

    let metadata = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, path = export_path, "Failed to stat file");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json("Failed to read file"))).into_response();
        }
    };
    let total_size = metadata.len();
    // `range` and `disposition` were computed above before the proxy check

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
            // Full file — stream to avoid buffering large files in memory
            let file = match tokio::fs::File::open(path).await {
                Ok(f) => f,
                Err(e) => {
                    warn!(error = %e, "Failed to open file");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
                }
            };

            let stream = ReaderStream::new(file);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type)
                .header(header::CONTENT_LENGTH, total_size.to_string())
                .header(header::CONTENT_DISPOSITION, &disposition)
                .header(header::ACCEPT_RANGES, "bytes")
                .body(Body::from_stream(stream))
                .unwrap()
        }
    }
}

/// GET /api/files/{file_id}/content — JWT-authenticated file content
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
    serve_file_content(&state, &doc_id, headers).await
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

/// Proxy a file content request to the source agent's internal file server.
async fn proxy_to_agent(
    state: &crate::state::AppState,
    doc: &serde_json::Value,
    export_path: &str,
    file_size: u64,
    range: Option<(u64, Option<u64>)>,
    mime_type: &str,
    disposition: &str,
) -> Response {
    let node_id = doc
        .get("source")
        .and_then(|s| s.get("node_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if node_id.is_empty() {
        return (StatusCode::NOT_FOUND, Json(error_json("File has no source node"))).into_response();
    }

    let node_doc = match state.db.get_document(&format!("node::{}", node_id)).await {
        Ok(d) => d,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, Json(error_json("Source node not found"))).into_response();
        }
    };

    let file_server_url = match node_doc.get("file_server_url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => {
            return (StatusCode::NOT_FOUND, Json(error_json("File not accessible: agent has no file server"))).into_response();
        }
    };
    let agent_token = match node_doc.get("agent_token").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return (StatusCode::NOT_FOUND, Json(error_json("File not accessible: agent has no token"))).into_response();
        }
    };

    let mut proxy_url = format!(
        "{}/internal/files/content?path={}",
        file_server_url,
        urlencoding::encode(export_path),
    );
    if let Some((start, end)) = range {
        let end_val = end.unwrap_or(file_size.saturating_sub(1));
        proxy_url.push_str(&format!("&start={}&end={}", start, end_val));
    }

    let client = reqwest::Client::new();
    let resp = match client
        .get(&proxy_url)
        .header("Authorization", format!("Bearer {}", agent_token))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, url = %proxy_url, "Failed to reach agent file server");
            return (StatusCode::BAD_GATEWAY, Json(error_json("Failed to reach agent"))).into_response();
        }
    };

    let proxy_status = resp.status();
    if !proxy_status.is_success() {
        return (StatusCode::BAD_GATEWAY, Json(error_json("Agent returned error"))).into_response();
    }

    let content_length = resp.content_length();
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::ACCEPT_RANGES, "bytes");

    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len.to_string());
    }

    if let Some((start, end)) = range {
        let end_val = end.unwrap_or(file_size.saturating_sub(1));
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end_val, file_size),
        );
    }

    builder.body(Body::from_stream(resp.bytes_stream())).unwrap()
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
