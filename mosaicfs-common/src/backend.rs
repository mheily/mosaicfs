/// Trait implemented by all storage backend adapters.
///
/// Each backend handles the raw I/O for a specific target type (S3, B2,
/// local directory, remote agent). The replication subsystem is responsible
/// for rule evaluation, scheduling, rate limiting, and state tracking;
/// the adapter is responsible only for uploading, downloading, deleting,
/// and listing objects.
#[async_trait::async_trait]
pub trait BackendAdapter: Send + Sync {
    /// Upload data to the backend at the given remote key.
    async fn upload(&self, remote_key: &str, data: bytes::Bytes) -> anyhow::Result<()>;

    /// Download an object from the backend by remote key.
    async fn download(&self, remote_key: &str) -> anyhow::Result<bytes::Bytes>;

    /// Delete an object from the backend by remote key.
    async fn delete(&self, remote_key: &str) -> anyhow::Result<()>;

    /// List all object keys under the given prefix.
    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>>;
}

/// Compute the remote key for a file given the backend prefix.
/// Scheme: `{prefix}/{file_uuid_8}/{filename}`
pub fn remote_key(prefix: &str, file_uuid: &str, filename: &str) -> String {
    let uuid8 = &file_uuid[..file_uuid.len().min(8)];
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        format!("{}/{}", uuid8, filename)
    } else {
        format!("{}/{}/{}", prefix, uuid8, filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_key() {
        assert_eq!(
            remote_key("mosaicfs/", "a3f92b1c-abcd-1234-5678-000000000000", "report.pdf"),
            "mosaicfs/a3f92b1c/report.pdf"
        );
        assert_eq!(
            remote_key("", "a3f92b1c-abcd-1234-5678-000000000000", "file.txt"),
            "a3f92b1c/file.txt"
        );
        assert_eq!(
            remote_key("backup/photos", "12345678-abcd", "img.jpg"),
            "backup/photos/12345678/img.jpg"
        );
    }
}
