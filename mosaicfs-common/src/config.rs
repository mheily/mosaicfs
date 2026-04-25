//! Unified TOML configuration for the `mosaicfs` binary.
//!
//! A single TOML file describes one node. The `[features]` block selects
//! which subsystems run. Each feature's dedicated section is required iff
//! that feature is enabled.
//!
//! Env vars override a small set of fields (mostly secrets and deployment
//! paths). The canonical names are preserved for compatibility with existing
//! container and systemd setups.

use std::path::PathBuf;

use serde::Deserialize;

/// Top-level node configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MosaicfsConfig {
    #[serde(default)]
    pub node: NodeConfig,
    pub features: FeaturesConfig,
    pub couchdb: CouchdbConfig,
    #[serde(default)]
    pub agent: Option<AgentFeatureConfig>,
    #[serde(default)]
    pub vfs: Option<VfsFeatureConfig>,
    #[serde(default)]
    pub web_ui: Option<WebUiFeatureConfig>,
    #[serde(default)]
    pub credentials: Option<CredentialsConfig>,
    #[serde(default)]
    pub secrets: SecretsConfig,
}

/// Selects the backend used to resolve node-level secrets.
///
/// - `inline` (default, all platforms) — secrets live in `[couchdb]` /
///   `[credentials]` in this file.
/// - `keychain` (macOS only) — secrets live in the macOS Keychain;
///   matching fields in the file may be empty.
#[derive(Debug, Clone, Deserialize)]
pub struct SecretsConfig {
    #[serde(default = "default_secrets_manager")]
    pub manager: String,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            manager: default_secrets_manager(),
        }
    }
}

fn default_secrets_manager() -> String {
    "inline".to_string()
}

/// Node-level access credentials.
///
/// `access_key_id` + `secret_key` authenticate this node to remote
/// `web_ui` peers. When the active secrets backend is `keychain`, both
/// fields may be left empty (or absent) in the TOML file — the values
/// live in the Keychain instead.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CredentialsConfig {
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_key: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeConfig {
    #[serde(default)]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeaturesConfig {
    #[serde(default)]
    pub agent: bool,
    #[serde(default)]
    pub vfs: bool,
    #[serde(default)]
    pub web_ui: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CouchdbConfig {
    pub url: String,
    pub user: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentFeatureConfig {
    pub watch_paths: Vec<PathBuf>,
    #[serde(default)]
    pub excluded_paths: Vec<PathBuf>,
    #[serde(default)]
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VfsFeatureConfig {
    pub mount_point: PathBuf,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebUiFeatureConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
    #[serde(default)]
    pub insecure_http: bool,
    #[serde(default)]
    pub developer_mode: bool,
    /// Unix domain socket path. When set the server binds here instead of
    /// the TCP `listen` address. Implies insecure (plain) HTTP.
    #[serde(default)]
    pub socket_path: Option<PathBuf>,
}

fn default_listen() -> String {
    "0.0.0.0:8443".to_string()
}

impl MosaicfsConfig {
    /// Parse a TOML file, apply env-var overrides, then validate.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
        let mut config: MosaicfsConfig = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", path.display(), e))?;
        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    /// Parse from a TOML string (testing and fixtures).
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut config: MosaicfsConfig = toml::from_str(s)?;
        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    /// Env vars that MosaicFS honours as overrides. Env wins over file for
    /// these fields so that container/systemd deployments can keep secrets
    /// out of the TOML file.
    ///
    /// - `COUCHDB_URL`, `COUCHDB_USER`, `COUCHDB_PASSWORD` → `[couchdb]`
    /// - `MOSAICFS_ACCESS_KEY_ID`, `MOSAICFS_SECRET_KEY` → `[credentials]`
    /// - `MOSAICFS_PORT` → appends to `[web_ui].listen`'s host when set
    /// - `MOSAICFS_DATA_DIR` → `[web_ui].data_dir`
    /// - `MOSAICFS_STATE_DIR` → `[agent].state_dir`
    /// - `MOSAICFS_INSECURE_HTTP` → `[web_ui].insecure_http`
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("COUCHDB_URL") {
            self.couchdb.url = v;
        }
        if let Ok(v) = std::env::var("COUCHDB_USER") {
            self.couchdb.user = v;
        }
        if let Ok(v) = std::env::var("COUCHDB_PASSWORD") {
            self.couchdb.password = v;
        }

        let access_key = std::env::var("MOSAICFS_ACCESS_KEY_ID").ok();
        let secret_key = std::env::var("MOSAICFS_SECRET_KEY").ok();
        if access_key.is_some() || secret_key.is_some() {
            let creds = self.credentials.get_or_insert_with(CredentialsConfig::default);
            if let Some(v) = access_key {
                creds.access_key_id = v;
            }
            if let Some(v) = secret_key {
                creds.secret_key = v;
            }
        }

        if let Some(web) = self.web_ui.as_mut() {
            if let Ok(v) = std::env::var("MOSAICFS_PORT") {
                if let Ok(port) = v.parse::<u16>() {
                    web.listen = rebind_port(&web.listen, port);
                }
            }
            if let Ok(v) = std::env::var("MOSAICFS_DATA_DIR") {
                web.data_dir = Some(PathBuf::from(v));
            }
            if let Ok(v) = std::env::var("MOSAICFS_INSECURE_HTTP") {
                web.insecure_http = v == "1" || v.eq_ignore_ascii_case("true");
            }
        }

        if let Some(agent) = self.agent.as_mut() {
            if let Ok(v) = std::env::var("MOSAICFS_STATE_DIR") {
                agent.state_dir = Some(PathBuf::from(v));
            }
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        match self.secrets.manager.as_str() {
            "inline" => {}
            "keychain" => {
                if !cfg!(target_os = "macos") {
                    anyhow::bail!(
                        "[secrets].manager = \"keychain\" is only supported on macOS; \
                         use \"inline\" (the default) on this platform"
                    );
                }
            }
            other => anyhow::bail!(
                "[secrets].manager = \"{other}\" is not recognized (expected \"inline\" or \"keychain\")"
            ),
        }

        // couchdb.url / user are required in inline mode; in keychain
        // mode the backend supplies them, so empty TOML fields are OK.
        if self.secrets.manager == "inline" {
            if self.couchdb.url.is_empty() {
                anyhow::bail!("[couchdb].url must not be empty");
            }
            if self.couchdb.user.is_empty() {
                anyhow::bail!("[couchdb].user must not be empty");
            }
        }

        if !(self.features.agent || self.features.vfs || self.features.web_ui) {
            anyhow::bail!("[features] must enable at least one of: agent, vfs, web_ui");
        }

        if self.features.agent {
            let agent = self
                .agent
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("[agent] section required when features.agent = true"))?;
            if agent.watch_paths.is_empty() {
                anyhow::bail!("[agent].watch_paths must contain at least one path");
            }
            for p in &agent.watch_paths {
                if !p.is_absolute() {
                    anyhow::bail!("[agent].watch_paths entry must be absolute: {}", p.display());
                }
            }
            for p in &agent.excluded_paths {
                if !p.is_absolute() {
                    anyhow::bail!(
                        "[agent].excluded_paths entry must be absolute: {}",
                        p.display()
                    );
                }
            }
        }

        if self.features.vfs {
            let vfs = self
                .vfs
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("[vfs] section required when features.vfs = true"))?;
            if !vfs.mount_point.is_absolute() {
                anyhow::bail!("[vfs].mount_point must be absolute: {}", vfs.mount_point.display());
            }
        }

        if self.features.web_ui {
            let web = self
                .web_ui
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("[web_ui] section required when features.web_ui = true"))?;
            if web.listen.is_empty() {
                anyhow::bail!("[web_ui].listen must not be empty");
            }
            if web.listen.parse::<std::net::SocketAddr>().is_err() {
                anyhow::bail!("[web_ui].listen is not a valid socket address: {}", web.listen);
            }
        }

        Ok(())
    }
}

/// Replace the port portion of a `host:port` string.
fn rebind_port(listen: &str, new_port: u16) -> String {
    match listen.rsplit_once(':') {
        Some((host, _)) => format!("{}:{}", host, new_port),
        None => format!("0.0.0.0:{}", new_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes env-mutating tests — otherwise one test's env writes
    /// leak into another test's `apply_env_overrides` call.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn minimal_couchdb() -> &'static str {
        r#"
[couchdb]
url = "http://localhost:5984"
user = "admin"
password = "pw"
"#
    }

    #[test]
    fn parse_full_config() {
        let toml = format!(
            r#"
[node]
node_id = "node-abc123"

[features]
agent = true
vfs = true
web_ui = true

[agent]
watch_paths = ["/data"]
excluded_paths = ["/data/.cache"]

[vfs]
mount_point = "/mnt/mosaicfs"

[web_ui]
listen = "0.0.0.0:8443"

{}
"#,
            minimal_couchdb()
        );
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let cfg = MosaicfsConfig::from_str(&toml).unwrap();
        assert_eq!(cfg.node.node_id.as_deref(), Some("node-abc123"));
        assert!(cfg.features.agent && cfg.features.vfs && cfg.features.web_ui);
        assert_eq!(cfg.agent.unwrap().watch_paths.len(), 1);
        assert_eq!(cfg.vfs.unwrap().mount_point, PathBuf::from("/mnt/mosaicfs"));
        assert_eq!(cfg.web_ui.unwrap().listen, "0.0.0.0:8443");
    }

    #[test]
    fn at_least_one_feature_required() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
agent = false
vfs = false
web_ui = false

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("at least one"), "got: {err}");
    }

    #[test]
    fn agent_requires_agent_section() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
agent = true

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("[agent] section required"), "got: {err}");
    }

    #[test]
    fn vfs_requires_absolute_mount_point() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
vfs = true

[vfs]
mount_point = "relative/mnt"

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("absolute"), "got: {err}");
    }

    #[test]
    fn web_ui_requires_valid_listen() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
web_ui = true

[web_ui]
listen = "not-a-socket"

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("socket address"), "got: {err}");
    }

    #[test]
    fn agent_rejects_relative_watch_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
agent = true

[agent]
watch_paths = ["relative/path"]

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("absolute"), "got: {err}");
    }

    #[test]
    fn env_overrides_couchdb_secrets() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        // SAFETY: guarded by ENV_LOCK — no other test reads env concurrently.
        unsafe {
            std::env::set_var("COUCHDB_URL", "http://from-env:5984");
            std::env::set_var("COUCHDB_USER", "envuser");
            std::env::set_var("COUCHDB_PASSWORD", "envpw");
        }
        let toml = r#"
[features]
agent = true

[agent]
watch_paths = ["/data"]

[couchdb]
url = "http://from-file:5984"
user = "fileuser"
password = "filepw"
"#;
        let cfg = MosaicfsConfig::from_str(toml).unwrap();
        assert_eq!(cfg.couchdb.url, "http://from-env:5984");
        assert_eq!(cfg.couchdb.user, "envuser");
        assert_eq!(cfg.couchdb.password, "envpw");
        clear_env();
    }

    #[test]
    fn secrets_manager_defaults_to_inline() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
agent = true

[agent]
watch_paths = ["/data"]

{}
"#,
            minimal_couchdb()
        );
        let cfg = MosaicfsConfig::from_str(&toml).unwrap();
        assert_eq!(cfg.secrets.manager, "inline");
    }

    #[test]
    fn secrets_manager_rejects_unknown_value() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
agent = true

[agent]
watch_paths = ["/data"]

[secrets]
manager = "vault"

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("not recognized"), "got: {err}");
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn secrets_manager_keychain_rejected_off_macos() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let toml = format!(
            r#"
[features]
agent = true

[agent]
watch_paths = ["/data"]

[secrets]
manager = "keychain"

{}
"#,
            minimal_couchdb()
        );
        let err = MosaicfsConfig::from_str(&toml).unwrap_err().to_string();
        assert!(err.contains("macOS"), "got: {err}");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn secrets_manager_keychain_allowed_on_macos() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        // On macOS the value parses; [couchdb] fields are allowed to be empty
        // because the keychain supplies them at runtime.
        let toml = r#"
[features]
agent = true

[agent]
watch_paths = ["/data"]

[secrets]
manager = "keychain"

[couchdb]
url = ""
user = ""
password = ""
"#;
        let cfg = MosaicfsConfig::from_str(toml).unwrap();
        assert_eq!(cfg.secrets.manager, "keychain");
    }

    #[test]
    fn rebind_port_replaces_port() {
        assert_eq!(rebind_port("0.0.0.0:8443", 9000), "0.0.0.0:9000");
        assert_eq!(rebind_port("127.0.0.1:1234", 5678), "127.0.0.1:5678");
    }

    fn clear_env() {
        // SAFETY: tests mutate process env; acceptable for unit tests.
        unsafe {
            for k in [
                "COUCHDB_URL",
                "COUCHDB_USER",
                "COUCHDB_PASSWORD",
                "MOSAICFS_PORT",
                "MOSAICFS_DATA_DIR",
                "MOSAICFS_STATE_DIR",
                "MOSAICFS_INSECURE_HTTP",
                "MOSAICFS_ACCESS_KEY_ID",
                "MOSAICFS_SECRET_KEY",
            ] {
                std::env::remove_var(k);
            }
        }
    }
}
