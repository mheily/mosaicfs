use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::warn;

use crate::couchdb::CouchError;
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

const TIMESTAMP_TOLERANCE_SECS: i64 = 300; // 5 minutes

/// Claims extracted from HMAC auth header
#[derive(Debug, Clone)]
pub struct HmacClaims {
    pub access_key_id: String,
}

/// Parse the HMAC authorization header.
/// Format: MOSAICFS-HMAC-SHA256 AccessKeyId=... Timestamp=... Signature=...
fn parse_hmac_header(header: &str) -> Option<(String, String, String)> {
    let header = header.strip_prefix("MOSAICFS-HMAC-SHA256")?;
    let header = header.trim();

    let mut access_key_id = None;
    let mut timestamp = None;
    let mut signature = None;

    for part in header.split_whitespace() {
        if let Some(val) = part.strip_prefix("AccessKeyId=") {
            access_key_id = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("Timestamp=") {
            timestamp = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("Signature=") {
            signature = Some(val.to_string());
        }
    }

    Some((access_key_id?, timestamp?, signature?))
}

/// Compute the expected HMAC signature
fn compute_signature(
    secret: &str,
    method: &str,
    path: &str,
    timestamp: &str,
    body_hash: &str,
) -> String {
    let canonical = format!("{}\n{}\n{}\n{}", method, path, timestamp, body_hash);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(canonical.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Compute SHA-256 hash of body bytes
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(data);
    hex::encode(hash)
}

/// Axum middleware that validates HMAC-signed requests on /api/agent/ endpoints
pub async fn hmac_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let (access_key_id, timestamp_str, signature) = match auth_header.as_deref().and_then(parse_hmac_header) {
        Some(parsed) => parsed,
        None => {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid HMAC authorization header")
                .into_response();
        }
    };

    // Validate timestamp
    let timestamp = match timestamp_str.parse::<DateTime<Utc>>() {
        Ok(t) => t,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Invalid timestamp format").into_response();
        }
    };

    let now = Utc::now();
    let diff = (now - timestamp).num_seconds().abs();
    if diff > TIMESTAMP_TOLERANCE_SECS {
        warn!(
            access_key_id = %access_key_id,
            diff_secs = diff,
            "HMAC timestamp rejected"
        );
        return (StatusCode::UNAUTHORIZED, "Timestamp out of range").into_response();
    }

    // Look up credential
    let cred_doc = match state
        .db
        .get_document(&format!("credential::{}", access_key_id))
        .await
    {
        Ok(doc) => doc,
        Err(CouchError::NotFound(_)) => {
            return (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response();
        }
        Err(e) => {
            warn!(error = %e, "Failed to look up credential");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Check enabled
    if cred_doc.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return (StatusCode::UNAUTHORIZED, "Credential is disabled").into_response();
    }

    // Get the secret key hash — we need the original secret to verify HMAC.
    // For HMAC, the agent uses the raw secret key (not the hash).
    // The server needs to verify using the same raw secret.
    // However, we only store the argon2id hash of the secret.
    //
    // Design note: HMAC auth requires a shared secret. Since we store argon2id hashes,
    // we use the secret_key_hash itself as the HMAC key — both sides derive it the same way.
    // Actually, per the architecture, the agent has the raw secret. We need to verify
    // the HMAC using the raw secret. But we don't store the raw secret.
    //
    // Resolution: Use a separate HMAC key derived from the secret at credential creation time.
    // For now, store an hmac_key field alongside the argon2id hash.
    let hmac_key = match cred_doc.get("hmac_key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => {
            warn!(access_key_id = %access_key_id, "Credential missing hmac_key");
            return (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response();
        }
    };

    // Read body for signature verification
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    // We need the body bytes for HMAC, then put them back
    let (parts, body) = req.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Request body too large").into_response();
        }
    };

    let body_hash = sha256_hex(&body_bytes);
    let expected_sig = compute_signature(&hmac_key, &method, &path, &timestamp_str, &body_hash);

    if !constant_time_eq(signature.as_bytes(), expected_sig.as_bytes()) {
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    // Reconstruct request with body
    let mut req = Request::from_parts(parts, Body::from(body_bytes));
    req.extensions_mut().insert(HmacClaims { access_key_id });

    next.run(req).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hmac_header() {
        let header = "MOSAICFS-HMAC-SHA256 AccessKeyId=MOSAICFS_ABC123 Timestamp=2025-11-14T09:22:00Z Signature=abcdef";
        let (akid, ts, sig) = parse_hmac_header(header).unwrap();
        assert_eq!(akid, "MOSAICFS_ABC123");
        assert_eq!(ts, "2025-11-14T09:22:00Z");
        assert_eq!(sig, "abcdef");
    }

    #[test]
    fn test_compute_signature() {
        let sig = compute_signature("secret", "POST", "/api/agent/heartbeat", "2025-11-14T09:22:00Z", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(!sig.is_empty());
        // Verify deterministic
        let sig2 = compute_signature("secret", "POST", "/api/agent/heartbeat", "2025-11-14T09:22:00Z", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sig, sig2);
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }
}
