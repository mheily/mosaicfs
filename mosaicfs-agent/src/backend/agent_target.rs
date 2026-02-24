//! Agent-to-agent replication backend.
//!
//! Replicates files to another MosaicFS agent by uploading via its
//! transfer/receive endpoint and downloading via its transfer endpoint.

use anyhow::{bail, Context};
use async_trait::async_trait;
use bytes::Bytes;
use tracing::debug;

use mosaicfs_common::backend::BackendAdapter;

/// Backend adapter that replicates to another MosaicFS agent node.
pub struct AgentAdapter {
    /// Base URL of the target agent's control plane (used to resolve transfer).
    target_agent_url: String,
    /// HMAC access key ID for agent-to-agent authentication.
    access_key_id: String,
    /// HMAC secret key for signing requests.
    secret_key: String,
    /// Path prefix on the target agent where files are stored.
    path_prefix: String,
    client: reqwest::Client,
}

impl AgentAdapter {
    pub fn new(
        target_agent_url: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_key: impl Into<String>,
        path_prefix: impl Into<String>,
    ) -> Self {
        Self {
            target_agent_url: target_agent_url.into(),
            access_key_id: access_key_id.into(),
            secret_key: secret_key.into(),
            path_prefix: path_prefix.into(),
            client: reqwest::Client::new(),
        }
    }

    fn upload_url(&self, remote_key: &str) -> String {
        format!(
            "{}/api/agent/replica-receive/{}",
            self.target_agent_url.trim_end_matches('/'),
            urlencoding::encode(remote_key)
        )
    }

    fn download_url(&self, remote_key: &str) -> String {
        format!(
            "{}/api/agent/replica-serve/{}",
            self.target_agent_url.trim_end_matches('/'),
            urlencoding::encode(remote_key)
        )
    }

    fn sign_request(&self, method: &str, path: &str, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let timestamp = chrono::Utc::now().timestamp().to_string();
        let body_hash = hex::encode(sha2::Sha256::digest(body));
        let canonical = format!("{}\n{}\n{}\n{}", method, path, timestamp, body_hash);

        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC key length ok");
        mac.update(canonical.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        format!(
            "MOSAICFS-HMAC-SHA256 AccessKeyId={} Timestamp={} Signature={}",
            self.access_key_id, timestamp, signature
        )
    }
}

#[async_trait]
impl BackendAdapter for AgentAdapter {
    async fn upload(&self, remote_key: &str, data: Bytes) -> anyhow::Result<()> {
        let url = self.upload_url(remote_key);
        let path = format!("/api/agent/replica-receive/{}", remote_key);
        let auth = self.sign_request("POST", &path, &data);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", auth)
            .header("Content-Type", "application/octet-stream")
            .body(data)
            .send()
            .await
            .context("Agent upload request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Agent upload failed: HTTP {} - {}", status, body);
        }

        debug!(key = %remote_key, "Agent target upload complete");
        Ok(())
    }

    async fn download(&self, remote_key: &str) -> anyhow::Result<Bytes> {
        let url = self.download_url(remote_key);
        let path = format!("/api/agent/replica-serve/{}", remote_key);
        let auth = self.sign_request("GET", &path, b"");

        let resp = self
            .client
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await
            .context("Agent download request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Agent download failed: HTTP {} - {}", status, body);
        }

        Ok(resp.bytes().await.context("Failed to read agent response")?)
    }

    async fn delete(&self, remote_key: &str) -> anyhow::Result<()> {
        // For agent targets, deletion is handled by the agent's own cleanup.
        // We notify via an API call but don't fail if the agent is unreachable.
        let url = format!(
            "{}/api/agent/replica-serve/{}",
            self.target_agent_url.trim_end_matches('/'),
            urlencoding::encode(remote_key)
        );
        let path = format!("/api/agent/replica-serve/{}", remote_key);
        let auth = self.sign_request("DELETE", &path, b"");

        let _ = self.client.delete(&url).header("Authorization", auth).send().await;
        debug!(key = %remote_key, "Agent target delete request sent");
        Ok(())
    }

    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let url = format!(
            "{}/api/agent/replica-list?prefix={}",
            self.target_agent_url.trim_end_matches('/'),
            urlencoding::encode(prefix)
        );
        let path = format!("/api/agent/replica-list?prefix={}", prefix);
        let auth = self.sign_request("GET", &path, b"");

        let resp = self
            .client
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await
            .context("Agent list request failed")?;

        if !resp.status().is_success() {
            bail!("Agent list failed: HTTP {}", resp.status());
        }

        let json: serde_json::Value = resp.json().await.context("Failed to parse agent list response")?;
        let keys = json["keys"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        Ok(keys)
    }
}
