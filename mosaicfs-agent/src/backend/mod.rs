pub mod agent_target;
pub mod directory;
pub mod s3;

use anyhow::bail;
use mosaicfs_common::backend::BackendAdapter;

use self::agent_target::AgentAdapter;
use self::directory::DirectoryAdapter;
use self::s3::{S3Adapter, S3Config};

/// Construct a backend adapter from a storage_backend CouchDB document.
///
/// Reads credentials from the resolved credential document if `credentials_doc`
/// is provided.
pub fn from_backend_doc(
    backend_doc: &serde_json::Value,
    credentials_doc: Option<&serde_json::Value>,
) -> anyhow::Result<Box<dyn BackendAdapter>> {
    let backend_type = backend_doc
        .get("backend")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let config = backend_doc.get("backend_config").cloned().unwrap_or_default();

    match backend_type {
        "s3" | "b2" => {
            let bucket = config
                .get("bucket")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let prefix = config
                .get("prefix")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let region = config
                .get("region")
                .and_then(|v| v.as_str())
                .unwrap_or("us-east-1")
                .to_string();
            let endpoint = config.get("endpoint").and_then(|v| v.as_str()).map(|s| s.to_string());
            let storage_class = config.get("storage_class").and_then(|v| v.as_str()).map(|s| s.to_string());

            // Extract credentials from the credential document
            let (access_key_id, secret_access_key) = if let Some(cred) = credentials_doc {
                let kid = cred.get("aws_access_key_id")
                    .or_else(|| cred.get("access_key_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let secret = cred.get("aws_secret_access_key")
                    .or_else(|| cred.get("secret_key"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (kid, secret)
            } else {
                // Fall back to environment variables
                (
                    std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
                    std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
                )
            };

            if access_key_id.is_empty() || secret_access_key.is_empty() {
                bail!("S3 credentials not found for backend '{}'", backend_type);
            }

            Ok(Box::new(S3Adapter::new(S3Config {
                bucket,
                prefix,
                region,
                endpoint,
                access_key_id,
                secret_access_key,
                storage_class,
            })))
        }
        "directory" => {
            let path = config
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("/tmp/mosaicfs-replication");
            Ok(Box::new(DirectoryAdapter::new(path)))
        }
        "agent" => {
            let node_id = config
                .get("node_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let path_prefix = config
                .get("path_prefix")
                .and_then(|v| v.as_str())
                .unwrap_or("/var/lib/mosaicfs/replicas");

            // For agent targets, the URL is constructed from node_id
            // In practice, the agent resolves node URLs via CouchDB
            let agent_url = config
                .get("agent_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let (access_key_id, secret_key) = if let Some(cred) = credentials_doc {
                let kid = cred.get("access_key_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let secret = cred.get("secret_key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                (kid, secret)
            } else {
                (String::new(), String::new())
            };

            Ok(Box::new(AgentAdapter::new(agent_url, access_key_id, secret_key, path_prefix)))
        }
        other => bail!("Unknown backend type: {}", other),
    }
}
