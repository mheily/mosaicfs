//! Secrets backend abstraction.
//!
//! Code that needs a node-level secret (CouchDB URL/user/password, the
//! node's own access key + secret key) calls [`SecretsBackend::get`]
//! instead of reading the parsed config struct directly. The selected
//! backend is picked at startup:
//!
//! - [`InlineBackend`] reads values from the parsed [`MosaicfsConfig`].
//!   Writes go back to the TOML file via `toml_edit`, preserving
//!   comments and formatting. This is the default on every platform.
//! - `KeychainBackend` (macOS-only, added in change 007 phase 3) stores
//!   values in the macOS Keychain so that the distributed config file
//!   contains no plaintext credentials.
//!
//! The set of known secret names is fixed and enumerated by
//! [`names`] / [`ALL_SECRET_NAMES`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Result, bail};

use crate::config::{CredentialsConfig, MosaicfsConfig};

/// The fully-qualified name of every secret MosaicFS knows about.
pub mod names {
    pub const COUCHDB_URL: &str = "couchdb.url";
    pub const COUCHDB_USER: &str = "couchdb.user";
    pub const COUCHDB_PASSWORD: &str = "couchdb.password";
    pub const CREDENTIALS_ACCESS_KEY_ID: &str = "credentials.access_key_id";
    pub const CREDENTIALS_SECRET_KEY: &str = "credentials.secret_key";
}

/// The complete, ordered list of secret names. Used by `secrets list`
/// / `secrets import` / validation helpers.
pub const ALL_SECRET_NAMES: &[&str] = &[
    names::COUCHDB_URL,
    names::COUCHDB_USER,
    names::COUCHDB_PASSWORD,
    names::CREDENTIALS_ACCESS_KEY_ID,
    names::CREDENTIALS_SECRET_KEY,
];

/// Trait implemented by every secrets-storage backend.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// async subsystems via `Arc<dyn SecretsBackend>`.
pub trait SecretsBackend: Send + Sync {
    /// Human-readable backend name (`"inline"`, `"keychain"`).
    fn kind(&self) -> &'static str;

    /// Return the value stored under `name`. Returns an error if the
    /// name is unknown or (for keychain) not present. Returning an
    /// empty string is legal — some deployments run CouchDB without a
    /// password, for instance.
    fn get(&self, name: &str) -> Result<String>;

    /// Store `value` under `name`. For the inline backend this
    /// rewrites the TOML file on disk; for keychain it writes the
    /// Keychain entry.
    fn set(&self, name: &str, value: &str) -> Result<()>;

    /// Names of secrets currently present (non-empty) in this backend.
    fn list(&self) -> Result<Vec<String>>;
}

/// Validate that `name` is one of the enumerated [`ALL_SECRET_NAMES`].
pub fn ensure_known(name: &str) -> Result<()> {
    if ALL_SECRET_NAMES.contains(&name) {
        Ok(())
    } else {
        bail!(
            "unknown secret '{name}' (known: {})",
            ALL_SECRET_NAMES.join(", ")
        )
    }
}

// ── Inline backend ──────────────────────────────────────────────────

/// Backend that serves values from the parsed [`MosaicfsConfig`] and
/// writes updates back to the TOML file.
pub struct InlineBackend {
    /// Path the TOML config was loaded from, if any. Required for
    /// `set` to succeed; tests may construct the backend without one.
    path: Option<PathBuf>,
    values: RwLock<HashMap<String, String>>,
}

impl InlineBackend {
    /// Snapshot every enumerated secret out of the parsed config.
    /// Missing fields (e.g. no `[credentials]` block) are stored as
    /// empty strings.
    pub fn from_config(cfg: &MosaicfsConfig, path: Option<PathBuf>) -> Self {
        let mut values = HashMap::new();
        values.insert(names::COUCHDB_URL.to_string(), cfg.couchdb.url.clone());
        values.insert(names::COUCHDB_USER.to_string(), cfg.couchdb.user.clone());
        values.insert(
            names::COUCHDB_PASSWORD.to_string(),
            cfg.couchdb.password.clone(),
        );
        let creds = cfg
            .credentials
            .clone()
            .unwrap_or_else(CredentialsConfig::default);
        values.insert(
            names::CREDENTIALS_ACCESS_KEY_ID.to_string(),
            creds.access_key_id,
        );
        values.insert(names::CREDENTIALS_SECRET_KEY.to_string(), creds.secret_key);
        Self {
            path,
            values: RwLock::new(values),
        }
    }
}

impl SecretsBackend for InlineBackend {
    fn kind(&self) -> &'static str {
        "inline"
    }

    fn get(&self, name: &str) -> Result<String> {
        ensure_known(name)?;
        let guard = self.values.read().expect("secrets lock poisoned");
        Ok(guard.get(name).cloned().unwrap_or_default())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        ensure_known(name)?;
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("inline backend has no file path; cannot persist"))?;
        write_inline_value(path, name, value)?;
        self.values
            .write()
            .expect("secrets lock poisoned")
            .insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>> {
        let guard = self.values.read().expect("secrets lock poisoned");
        let mut out: Vec<String> = ALL_SECRET_NAMES
            .iter()
            .filter(|n| guard.get(**n).map_or(false, |v| !v.is_empty()))
            .map(|n| (*n).to_string())
            .collect();
        out.sort();
        Ok(out)
    }
}

/// Rewrite the TOML file at `path` so that the dotted `section.key`
/// (e.g. `couchdb.password`) takes `value`. Preserves comments and
/// unrelated keys.
fn write_inline_value(path: &Path, name: &str, value: &str) -> Result<()> {
    use toml_edit::{DocumentMut, Item, Table, value as te_value};

    let (section, key) = name
        .split_once('.')
        .ok_or_else(|| anyhow::anyhow!("secret name '{name}' is not dotted (section.key)"))?;

    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let mut doc: DocumentMut = content
        .parse()
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;

    if doc.get(section).is_none() {
        doc.insert(section, Item::Table(Table::new()));
    }
    let table = doc[section]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[{section}] is not a table in {}", path.display()))?;
    table[key] = te_value(value);

    std::fs::write(path, doc.to_string())
        .map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    Ok(())
}

// ── Factory ─────────────────────────────────────────────────────────

/// Open the secrets backend appropriate for this config.
///
/// Phase 1: always returns [`InlineBackend`]. Phase 2 introduces a
/// `[secrets]` config block that selects the backend.
pub fn open(
    cfg: &MosaicfsConfig,
    config_path: Option<&Path>,
) -> Result<Arc<dyn SecretsBackend>> {
    Ok(Arc::new(InlineBackend::from_config(
        cfg,
        config_path.map(|p| p.to_path_buf()),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_config() -> MosaicfsConfig {
        let toml = r#"
[features]
agent = true

[agent]
watch_paths = ["/data"]

[couchdb]
url = "http://localhost:5984"
user = "admin"
password = "pw"

[credentials]
access_key_id = "MOSAICFS_TEST"
secret_key = "mosaicfs_testsecret"
"#;
        MosaicfsConfig::from_str(toml).unwrap()
    }

    #[test]
    fn inline_get_reads_config_values() {
        let cfg = sample_config();
        let backend = InlineBackend::from_config(&cfg, None);
        assert_eq!(backend.get(names::COUCHDB_URL).unwrap(), "http://localhost:5984");
        assert_eq!(backend.get(names::COUCHDB_PASSWORD).unwrap(), "pw");
        assert_eq!(
            backend.get(names::CREDENTIALS_ACCESS_KEY_ID).unwrap(),
            "MOSAICFS_TEST"
        );
    }

    #[test]
    fn inline_get_rejects_unknown_names() {
        let backend = InlineBackend::from_config(&sample_config(), None);
        let err = backend.get("couchdb.not_a_field").unwrap_err().to_string();
        assert!(err.contains("unknown secret"), "got: {err}");
    }

    #[test]
    fn inline_get_returns_empty_for_absent_credentials_block() {
        let toml = r#"
[features]
web_ui = true

[web_ui]
listen = "0.0.0.0:8443"

[couchdb]
url = "http://localhost:5984"
user = "admin"
password = "pw"
"#;
        let cfg = MosaicfsConfig::from_str(toml).unwrap();
        let backend = InlineBackend::from_config(&cfg, None);
        assert_eq!(backend.get(names::CREDENTIALS_SECRET_KEY).unwrap(), "");
    }

    #[test]
    fn inline_list_skips_empty_values() {
        let toml = r#"
[features]
web_ui = true

[web_ui]
listen = "0.0.0.0:8443"

[couchdb]
url = "http://localhost:5984"
user = "admin"
password = ""
"#;
        let cfg = MosaicfsConfig::from_str(toml).unwrap();
        let backend = InlineBackend::from_config(&cfg, None);
        let present = backend.list().unwrap();
        assert!(present.contains(&names::COUCHDB_URL.to_string()));
        assert!(present.contains(&names::COUCHDB_USER.to_string()));
        assert!(!present.contains(&names::COUCHDB_PASSWORD.to_string()));
    }

    #[test]
    fn inline_set_rewrites_toml_preserving_comments() {
        let original = r#"# top comment
[features]
agent = true

[agent]
watch_paths = ["/data"]

# CouchDB admin creds
[couchdb]
url = "http://localhost:5984"
user = "admin"
password = "old"
"#;
        let tmp = tempfile_path("mosaicfs-secrets-inline-set.toml");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(original.as_bytes()).unwrap();
        }
        let cfg = MosaicfsConfig::load(&tmp).unwrap();
        let backend = InlineBackend::from_config(&cfg, Some(tmp.clone()));
        backend.set(names::COUCHDB_PASSWORD, "rotated").unwrap();
        let after = std::fs::read_to_string(&tmp).unwrap();
        assert!(after.contains("password = \"rotated\""));
        assert!(after.contains("# top comment"));
        assert!(after.contains("# CouchDB admin creds"));
        assert_eq!(backend.get(names::COUCHDB_PASSWORD).unwrap(), "rotated");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn inline_set_errors_without_path() {
        let backend = InlineBackend::from_config(&sample_config(), None);
        let err = backend
            .set(names::COUCHDB_PASSWORD, "x")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no file path"), "got: {err}");
    }

    fn tempfile_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "{}-{}-{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        p
    }
}
