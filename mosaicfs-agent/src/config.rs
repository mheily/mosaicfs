use serde::Deserialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub control_plane_url: String,
    #[serde(default)]
    pub node_id: Option<String>,
    pub watch_paths: Vec<PathBuf>,
    #[serde(default)]
    pub excluded_paths: Vec<PathBuf>,
    pub access_key_id: String,
    pub secret_key: String,
}

impl AgentConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
        let config: AgentConfig = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", path.display(), e))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.control_plane_url.is_empty() {
            anyhow::bail!("control_plane_url must not be empty");
        }
        if self.watch_paths.is_empty() {
            anyhow::bail!("watch_paths must contain at least one path");
        }
        for p in &self.watch_paths {
            if !p.is_absolute() {
                anyhow::bail!("watch_path must be absolute: {}", p.display());
            }
        }
        for p in &self.excluded_paths {
            if !p.is_absolute() {
                anyhow::bail!("excluded_path must be absolute: {}", p.display());
            }
        }
        if self.access_key_id.is_empty() {
            anyhow::bail!("access_key_id must not be empty");
        }
        if self.secret_key.is_empty() {
            anyhow::bail!("secret_key must not be empty");
        }
        Ok(())
    }

    /// Resolve node_id: read from file, or generate and persist on first run.
    pub fn resolve_node_id(&mut self, state_dir: &Path) -> anyhow::Result<String> {
        if let Some(ref id) = self.node_id {
            return Ok(id.clone());
        }

        let node_id_file = state_dir.join("node_id");
        if node_id_file.exists() {
            let id = std::fs::read_to_string(&node_id_file)?.trim().to_string();
            self.node_id = Some(id.clone());
            return Ok(id);
        }

        let id = format!("node-{}", &Uuid::new_v4().to_string()[..8]);
        std::fs::create_dir_all(state_dir)?;
        std::fs::write(&node_id_file, &id)?;
        tracing::info!(node_id = %id, "Generated new node_id");
        self.node_id = Some(id.clone());
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_config() {
        let toml_str = r#"
control_plane_url = "https://localhost:8443"
watch_paths = ["/home/user/documents"]
excluded_paths = ["/home/user/documents/.cache"]
access_key_id = "MOSAICFS_abc123"
secret_key = "secret123"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        config.validate().unwrap();
        assert_eq!(config.watch_paths.len(), 1);
        assert_eq!(config.excluded_paths.len(), 1);
    }

    #[test]
    fn test_missing_watch_paths() {
        let toml_str = r#"
control_plane_url = "https://localhost:8443"
watch_paths = []
access_key_id = "MOSAICFS_abc123"
secret_key = "secret123"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_relative_watch_path_rejected() {
        let toml_str = r#"
control_plane_url = "https://localhost:8443"
watch_paths = ["relative/path"]
access_key_id = "MOSAICFS_abc123"
secret_key = "secret123"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert!(config.validate().is_err());
    }
}
