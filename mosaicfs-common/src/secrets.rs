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

// ── Keychain backend (macOS) ────────────────────────────────────────

/// Service name used for every MosaicFS entry in the macOS Keychain.
/// Every secret name becomes one entry under this service.
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "mosaicfs";

#[cfg(target_os = "macos")]
pub struct KeychainBackend {
    service: String,
}

#[cfg(target_os = "macos")]
impl KeychainBackend {
    pub fn new() -> Self {
        Self {
            service: KEYCHAIN_SERVICE.to_string(),
        }
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, name)
            .map_err(|e| anyhow::anyhow!("keychain open '{name}': {e}"))
    }
}

#[cfg(target_os = "macos")]
impl Default for KeychainBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl SecretsBackend for KeychainBackend {
    fn kind(&self) -> &'static str {
        "keychain"
    }

    fn get(&self, name: &str) -> Result<String> {
        ensure_known(name)?;
        let entry = self.entry(name)?;
        entry
            .get_password()
            .map_err(|e| anyhow::anyhow!("keychain read '{name}': {e}"))
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        ensure_known(name)?;
        let entry = self.entry(name)?;
        entry
            .set_password(value)
            .map_err(|e| anyhow::anyhow!("keychain write '{name}': {e}"))
    }

    fn list(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for name in ALL_SECRET_NAMES {
            let entry = self.entry(name)?;
            match entry.get_password() {
                Ok(v) if !v.is_empty() => out.push((*name).to_string()),
                Ok(_) => {}
                Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(anyhow::anyhow!("keychain list '{name}': {e}")),
            }
        }
        Ok(out)
    }
}

// ── Inline TOML helpers (used by the `secrets import` subcommand) ────

/// Read every known secret directly from the TOML file at `path`,
/// bypassing the active backend. Returns only the names whose fields
/// are present and non-empty.
pub fn read_inline_from_file(path: &Path) -> Result<Vec<(String, String)>> {
    use toml_edit::DocumentMut;
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let doc: DocumentMut = content
        .parse()
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for name in ALL_SECRET_NAMES {
        let (section, key) = name.split_once('.').expect("all names are dotted");
        if let Some(table) = doc.get(section).and_then(|i| i.as_table()) {
            if let Some(v) = table.get(key).and_then(|i| i.as_str()) {
                if !v.is_empty() {
                    out.push(((*name).to_string(), v.to_string()));
                }
            }
        }
    }
    Ok(out)
}

/// Blank (set to the empty string) the named secret fields in the TOML
/// file at `path`. Preserves comments and unrelated keys. Used by
/// `secrets import` after a successful migration to the keychain.
pub fn blank_inline_in_file(path: &Path, names: &[&str]) -> Result<()> {
    use toml_edit::{DocumentMut, value as te_value};
    for n in names {
        ensure_known(n)?;
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let mut doc: DocumentMut = content
        .parse()
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    for name in names {
        let (section, key) = name.split_once('.').expect("all names are dotted");
        if let Some(table) = doc.get_mut(section).and_then(|i| i.as_table_mut()) {
            if table.contains_key(key) {
                table[key] = te_value("");
            }
        }
    }
    std::fs::write(path, doc.to_string())
        .map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    Ok(())
}

// ── Factory ─────────────────────────────────────────────────────────

/// Open the secrets backend appropriate for this config.
///
/// The backend is picked by `[secrets].manager`:
///
/// - `"inline"` — always available. Reads from the parsed config, writes
///   back to the TOML file.
/// - `"keychain"` — macOS-only. Reads/writes the macOS Keychain; the
///   actual implementation is wired in change 007 phase 3.
pub fn open(
    cfg: &MosaicfsConfig,
    config_path: Option<&Path>,
) -> Result<Arc<dyn SecretsBackend>> {
    match cfg.secrets.manager.as_str() {
        "inline" => Ok(Arc::new(InlineBackend::from_config(
            cfg,
            config_path.map(|p| p.to_path_buf()),
        ))),
        "keychain" => {
            #[cfg(target_os = "macos")]
            {
                Ok(Arc::new(KeychainBackend::new()))
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = config_path;
                bail!(
                    "secrets.manager = \"keychain\" is only available on macOS"
                )
            }
        }
        other => bail!("unknown secrets.manager: \"{other}\""),
    }
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

    #[test]
    fn read_inline_from_file_skips_empty_and_missing() {
        let toml = r#"
[secrets]
manager = "inline"

[couchdb]
url = "http://localhost:5984"
user = "admin"
password = ""

[credentials]
access_key_id = "MOSAICFS_X"
# secret_key omitted entirely
"#;
        let tmp = tempfile_path("mosaicfs-secrets-read-inline.toml");
        std::fs::write(&tmp, toml).unwrap();
        let got = read_inline_from_file(&tmp).unwrap();
        let map: std::collections::HashMap<_, _> = got.into_iter().collect();
        assert_eq!(map.get(names::COUCHDB_URL).unwrap(), "http://localhost:5984");
        assert_eq!(map.get(names::COUCHDB_USER).unwrap(), "admin");
        assert!(!map.contains_key(names::COUCHDB_PASSWORD));
        assert_eq!(map.get(names::CREDENTIALS_ACCESS_KEY_ID).unwrap(), "MOSAICFS_X");
        assert!(!map.contains_key(names::CREDENTIALS_SECRET_KEY));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn blank_inline_in_file_sets_fields_to_empty_string() {
        let toml = r#"# node config
[secrets]
manager = "keychain"

[couchdb]
url = "http://localhost:5984"
user = "admin"
password = "filepw"

[credentials]
access_key_id = "MOSAICFS_A"
secret_key = "mosaicfs_s"
"#;
        let tmp = tempfile_path("mosaicfs-secrets-blank.toml");
        std::fs::write(&tmp, toml).unwrap();
        blank_inline_in_file(
            &tmp,
            &[names::COUCHDB_PASSWORD, names::CREDENTIALS_SECRET_KEY],
        )
        .unwrap();
        let after = std::fs::read_to_string(&tmp).unwrap();
        assert!(after.contains("password = \"\""));
        assert!(after.contains("secret_key = \"\""));
        assert!(after.contains("user = \"admin\"")); // untouched
        assert!(after.contains("# node config")); // comment preserved
        let _ = std::fs::remove_file(&tmp);
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
