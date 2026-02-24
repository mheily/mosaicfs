//! S3-compatible backend adapter.
//!
//! Handles AWS S3 and Backblaze B2 (S3-compatible API). Uses
//! reqwest with manual AWS Signature V4 signing so no SDK dependency is needed.

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use mosaicfs_common::backend::BackendAdapter;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct S3Config {
    pub bucket: String,
    pub prefix: String,
    pub region: String,
    pub endpoint: Option<String>, // Custom endpoint for B2 or other S3-compatible APIs
    pub access_key_id: String,
    pub secret_access_key: String,
    pub storage_class: Option<String>,
}

pub struct S3Adapter {
    config: S3Config,
    client: Client,
}

impl S3Adapter {
    pub fn new(config: S3Config) -> Self {
        Self {
            config,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    fn endpoint(&self) -> String {
        if let Some(ep) = &self.config.endpoint {
            ep.clone()
        } else {
            format!(
                "https://s3.{}.amazonaws.com/{}",
                self.config.region, self.config.bucket
            )
        }
    }

    fn object_url(&self, key: &str) -> String {
        format!(
            "{}/{}",
            self.endpoint().trim_end_matches('/'),
            urlencoding::encode(key)
        )
    }

    /// Compute AWS Signature V4 for a request.
    fn sign(
        &self,
        method: &str,
        path: &str,
        query: &str,
        headers: &BTreeMap<String, String>,
        body_hash: &str,
        date_time: &str,
        date: &str,
    ) -> String {
        // Canonical request
        let canonical_headers: String = headers
            .iter()
            .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
            .collect();
        let signed_headers: String = headers.keys().cloned().collect::<Vec<_>>().join(";");

        let canonical_request = format!(
            "{}\n/{}\n{}\n{}\n{}\n{}",
            method, path, query, canonical_headers, signed_headers, body_hash
        );

        // String to sign
        let cr_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let credential_scope = format!("{}/{}/s3/aws4_request", date, self.config.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            date_time, credential_scope, cr_hash
        );

        // Signing key
        let signing_key = derive_signing_key(
            &self.config.secret_access_key,
            date,
            &self.config.region,
        );

        let mut mac = HmacSha256::new_from_slice(&signing_key).expect("HMAC key length ok");
        mac.update(string_to_sign.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        format!(
            "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
            self.config.access_key_id, credential_scope, signed_headers, signature
        )
    }
}

fn derive_signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let key = format!("AWS4{}", secret);
    let k_date = hmac_sha256(key.as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length ok");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn body_hash(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[async_trait]
impl BackendAdapter for S3Adapter {
    async fn upload(&self, remote_key: &str, data: Bytes) -> anyhow::Result<()> {
        let now = Utc::now();
        let date_time = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let host = url_host(&self.endpoint());
        let path = format!("{}", remote_key);
        let body_hash_str = body_hash(&data);

        let mut headers = BTreeMap::new();
        headers.insert("content-length".to_string(), data.len().to_string());
        headers.insert("content-type".to_string(), "application/octet-stream".to_string());
        headers.insert("host".to_string(), host.clone());
        headers.insert("x-amz-content-sha256".to_string(), body_hash_str.clone());
        headers.insert("x-amz-date".to_string(), date_time.clone());

        if let Some(ref sc) = self.config.storage_class {
            headers.insert("x-amz-storage-class".to_string(), sc.clone());
        }

        let auth = self.sign("PUT", &path, "", &headers, &body_hash_str, &date_time, &date);

        let url = self.object_url(remote_key);
        let mut req = self
            .client
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .header("x-amz-date", &date_time)
            .header("x-amz-content-sha256", &body_hash_str)
            .header("Authorization", &auth);

        if let Some(ref sc) = self.config.storage_class {
            req = req.header("x-amz-storage-class", sc);
        }

        let resp = req.body(data).send().await.context("S3 PUT request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("S3 PUT failed: HTTP {} - {}", status, body);
        }

        debug!(key = %remote_key, "S3 upload complete");
        Ok(())
    }

    async fn download(&self, remote_key: &str) -> anyhow::Result<Bytes> {
        let now = Utc::now();
        let date_time = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let host = url_host(&self.endpoint());
        let path = format!("{}", remote_key);
        let empty_hash = body_hash(b"");

        let mut headers = BTreeMap::new();
        headers.insert("host".to_string(), host.clone());
        headers.insert("x-amz-content-sha256".to_string(), empty_hash.clone());
        headers.insert("x-amz-date".to_string(), date_time.clone());

        let auth = self.sign("GET", &path, "", &headers, &empty_hash, &date_time, &date);

        let url = self.object_url(remote_key);
        let resp = self
            .client
            .get(&url)
            .header("x-amz-date", &date_time)
            .header("x-amz-content-sha256", &empty_hash)
            .header("Authorization", &auth)
            .send()
            .await
            .context("S3 GET request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("S3 GET failed: HTTP {} - {}", status, body);
        }

        Ok(resp.bytes().await.context("Failed to read S3 response body")?)
    }

    async fn delete(&self, remote_key: &str) -> anyhow::Result<()> {
        let now = Utc::now();
        let date_time = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let host = url_host(&self.endpoint());
        let path = format!("{}", remote_key);
        let empty_hash = body_hash(b"");

        let mut headers = BTreeMap::new();
        headers.insert("host".to_string(), host.clone());
        headers.insert("x-amz-content-sha256".to_string(), empty_hash.clone());
        headers.insert("x-amz-date".to_string(), date_time.clone());

        let auth = self.sign("DELETE", &path, "", &headers, &empty_hash, &date_time, &date);

        let url = self.object_url(remote_key);
        let resp = self
            .client
            .delete(&url)
            .header("x-amz-date", &date_time)
            .header("x-amz-content-sha256", &empty_hash)
            .header("Authorization", &auth)
            .send()
            .await
            .context("S3 DELETE request failed")?;

        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("S3 DELETE failed: HTTP {} - {}", status, body);
        }

        debug!(key = %remote_key, "S3 delete complete");
        Ok(())
    }

    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let now = Utc::now();
        let date_time = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let host = url_host(&self.endpoint());
        let empty_hash = body_hash(b"");
        let query = format!(
            "list-type=2&prefix={}",
            urlencoding::encode(prefix)
        );

        let mut headers = BTreeMap::new();
        headers.insert("host".to_string(), host.clone());
        headers.insert("x-amz-content-sha256".to_string(), empty_hash.clone());
        headers.insert("x-amz-date".to_string(), date_time.clone());

        let auth = self.sign("GET", "", &query, &headers, &empty_hash, &date_time, &date);

        let url = format!("{}/?{}", self.endpoint().trim_end_matches('/'), query);
        let resp = self
            .client
            .get(&url)
            .header("x-amz-date", &date_time)
            .header("x-amz-content-sha256", &empty_hash)
            .header("Authorization", &auth)
            .send()
            .await
            .context("S3 LIST request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("S3 LIST failed: HTTP {} - {}", status, body);
        }

        // Parse XML response (minimal - extract <Key> elements)
        let body = resp.text().await.context("Failed to read S3 list response")?;
        let keys = parse_list_keys(&body);
        Ok(keys)
    }
}

/// Extract <Key>…</Key> values from S3 ListObjectsV2 XML response.
fn parse_list_keys(xml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut remaining = xml;
    while let Some(start) = remaining.find("<Key>") {
        remaining = &remaining[start + 5..];
        if let Some(end) = remaining.find("</Key>") {
            keys.push(remaining[..end].to_string());
            remaining = &remaining[end + 6..];
        }
    }
    keys
}

/// Extract the host part from a URL for use in signing.
fn url_host(url: &str) -> String {
    // Strip scheme and path, return just host[:port]
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme.split('/').next().unwrap_or(without_scheme).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list_keys() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Contents><Key>prefix/abc12345/file.txt</Key></Contents>
  <Contents><Key>prefix/def67890/photo.jpg</Key></Contents>
</ListBucketResult>"#;
        let keys = parse_list_keys(xml);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "prefix/abc12345/file.txt");
        assert_eq!(keys[1], "prefix/def67890/photo.jpg");
    }

    #[test]
    fn test_url_host() {
        assert_eq!(url_host("https://s3.us-east-1.amazonaws.com/bucket"), "s3.us-east-1.amazonaws.com");
        assert_eq!(url_host("https://s3.us-east-1.amazonaws.com/bucket/key/path"), "s3.us-east-1.amazonaws.com");
        assert_eq!(url_host("http://localhost:9000"), "localhost:9000");
    }

    #[test]
    fn test_hmac_sha256() {
        // Sanity check: produce a non-empty result
        let result = hmac_sha256(b"secret", b"data");
        assert_eq!(result.len(), 32);
    }
}
