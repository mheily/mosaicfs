use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use rand::Rng;

use crate::couchdb::{CouchClient, CouchError};

/// Generate a new access key ID in format MOSAICFS_{16_hex_chars}
pub fn generate_access_key_id() -> String {
    let hex_part: String = (0..16)
        .map(|_| format!("{:X}", rand::thread_rng().gen::<u8>() % 16))
        .collect();
    format!("MOSAICFS_{}", hex_part)
}

/// Generate a secret key: mosaicfs_ followed by 43 url-safe base64 chars
pub fn generate_secret_key() -> String {
    let random_bytes: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &random_bytes,
    );
    format!("mosaicfs_{}", encoded)
}

/// Hash a secret key with Argon2id
pub fn hash_secret(secret: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash secret: {}", e))?;
    Ok(hash.to_string())
}

/// Verify a secret against an Argon2id hash
pub fn verify_secret(secret: &str, hash: &str) -> bool {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &parsed_hash)
        .is_ok()
}

/// Generate an HMAC key derived from the secret (stored alongside the hash)
pub fn generate_hmac_key(secret: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(b"mosaicfs-hmac-derivation")
        .expect("HMAC key derivation");
    mac.update(secret.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Create a new credential and return the secret (shown once)
pub async fn create_credential(
    db: &CouchClient,
    name: &str,
) -> anyhow::Result<(String, String)> {
    let access_key_id = generate_access_key_id();
    let secret_key = generate_secret_key();
    let secret_hash = hash_secret(&secret_key)?;
    let hmac_key = generate_hmac_key(&secret_key);

    let doc = serde_json::json!({
        "_id": format!("credential::{}", access_key_id),
        "type": "credential",
        "access_key_id": access_key_id,
        "secret_key_hash": secret_hash,
        "hmac_key": hmac_key,
        "name": name,
        "enabled": true,
        "created_at": Utc::now().to_rfc3339(),
        "permissions": { "scope": "full" },
    });

    db.put_document(&format!("credential::{}", access_key_id), &doc)
        .await?;

    Ok((access_key_id, secret_key))
}

/// List all credentials (without secret hashes)
pub async fn list_credentials(db: &CouchClient) -> anyhow::Result<Vec<serde_json::Value>> {
    let resp = db.all_docs_by_prefix("credential::", true).await?;
    Ok(resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let mut doc = row.doc?;
            // Remove sensitive fields
            doc.as_object_mut().map(|o| {
                o.remove("secret_key_hash");
                o.remove("hmac_key");
                o.remove("_rev");
            });
            Some(doc)
        })
        .collect())
}

/// Get a single credential (without secret hash)
pub async fn get_credential(
    db: &CouchClient,
    access_key_id: &str,
) -> Result<serde_json::Value, CouchError> {
    let mut doc = db
        .get_document(&format!("credential::{}", access_key_id))
        .await?;
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("secret_key_hash");
        obj.remove("hmac_key");
        obj.remove("_rev");
    }
    Ok(doc)
}

/// Update credential (name or enabled status)
pub async fn update_credential(
    db: &CouchClient,
    access_key_id: &str,
    updates: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut doc = db
        .get_document(&format!("credential::{}", access_key_id))
        .await?;

    if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
        doc["name"] = serde_json::Value::String(name.to_string());
    }
    if let Some(enabled) = updates.get("enabled").and_then(|v| v.as_bool()) {
        doc["enabled"] = serde_json::Value::Bool(enabled);
    }

    db.put_document(&format!("credential::{}", access_key_id), &doc)
        .await?;
    Ok(())
}

/// Delete (revoke) a credential
pub async fn delete_credential(
    db: &CouchClient,
    access_key_id: &str,
) -> anyhow::Result<()> {
    let doc = db
        .get_document(&format!("credential::{}", access_key_id))
        .await?;
    let rev = doc
        .get("_rev")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing _rev"))?;
    db.delete_document(&format!("credential::{}", access_key_id), rev)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_key_id_format() {
        let key = generate_access_key_id();
        assert!(key.starts_with("MOSAICFS_"));
        assert_eq!(key.len(), "MOSAICFS_".len() + 16);
    }

    #[test]
    fn test_secret_key_format() {
        let secret = generate_secret_key();
        assert!(secret.starts_with("mosaicfs_"));
    }

    #[test]
    fn test_hash_and_verify() {
        let secret = generate_secret_key();
        let hash = hash_secret(&secret).unwrap();
        assert!(verify_secret(&secret, &hash));
        assert!(!verify_secret("wrong_secret", &hash));
    }

    #[test]
    fn test_hmac_key_deterministic() {
        let secret = "test_secret";
        let key1 = generate_hmac_key(secret);
        let key2 = generate_hmac_key(secret);
        assert_eq!(key1, key2);
    }
}
