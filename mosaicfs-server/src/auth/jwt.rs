use std::path::Path;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::state::AppState;

const JWT_EXPIRY_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // access_key_id
    pub exp: i64,
    pub iat: i64,
    pub jti: String, // unique token ID for revocation
}

/// Generate or load the JWT signing secret from disk
pub fn ensure_jwt_secret(data_dir: &Path) -> anyhow::Result<Vec<u8>> {
    let secret_path = data_dir.join("jwt_secret");
    if secret_path.exists() {
        Ok(std::fs::read(&secret_path)?)
    } else {
        let secret: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
        std::fs::write(&secret_path, &secret)?;
        info!("Generated new JWT signing secret");
        Ok(secret)
    }
}

/// Issue a new JWT for a given access_key_id
pub fn issue_token(secret: &[u8], access_key_id: &str) -> anyhow::Result<(String, i64)> {
    let now = Utc::now();
    let exp = now + Duration::hours(JWT_EXPIRY_HOURS);
    let claims = Claims {
        sub: access_key_id.to_string(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: uuid::Uuid::new_v4().to_string(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )?;

    Ok((token, exp.timestamp()))
}

/// Validate a JWT and return claims
pub fn validate_token(secret: &[u8], token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

/// Axum middleware that validates JWT Bearer tokens
pub async fn jwt_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid Authorization header")
                .into_response();
        }
    };

    let claims = match validate_token(&state.jwt_secret, token) {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response();
        }
    };

    // Check revocation
    {
        let revoked = state.revoked_tokens.lock().unwrap();
        if revoked.contains(&claims.jti) {
            return (StatusCode::UNAUTHORIZED, "Token has been revoked").into_response();
        }
    }

    // Store claims in request extensions for handlers
    req.extensions_mut().insert(claims);
    next.run(req).await
}
