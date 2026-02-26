use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tracing::{info, warn};

pub struct FileServerState {
    pub agent_token: String,
}

#[derive(Deserialize)]
pub struct ContentQuery {
    pub path: String,
    pub start: Option<u64>,
    pub end: Option<u64>,
}

pub async fn start(token: String, port: u16) {
    let state = Arc::new(FileServerState { agent_token: token });
    let app = Router::new()
        .route("/internal/files/content", get(serve_content))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "Agent file server listening");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            warn!(error = %e, "Failed to bind agent file server");
            return;
        }
    };
    if let Err(e) = axum::serve(listener, app).await {
        warn!(error = %e, "Agent file server error");
    }
}

async fn serve_content(
    State(state): State<Arc<FileServerState>>,
    headers: HeaderMap,
    Query(query): Query<ContentQuery>,
) -> Response {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    if auth != Some(state.agent_token.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let path = std::path::Path::new(&query.path);
    if !path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let total_size = match tokio::fs::metadata(path).await {
        Ok(m) => m.len(),
        Err(e) => {
            warn!(error = %e, path = %path.display(), "Failed to stat file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "Failed to open file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let (Some(start), Some(end)) = (query.start, query.end) {
        use tokio::io::AsyncSeekExt;
        if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
            warn!(error = %e, "Failed to seek");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let len = end - start + 1;
        let mut buf = vec![0u8; len as usize];
        if let Err(e) = file.read_exact(&mut buf).await {
            warn!(error = %e, "Failed to read range");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_LENGTH, len.to_string())
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, total_size),
            )
            .body(Body::from(buf))
            .unwrap()
    } else {
        let mut buf = Vec::with_capacity(total_size as usize);
        if let Err(e) = file.read_to_end(&mut buf).await {
            warn!(error = %e, "Failed to read file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, buf.len().to_string())
            .body(Body::from(buf))
            .unwrap()
    }
}
