use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub couchdb_url: String,
    #[serde(default)]
    pub couchdb_user: String,
    #[serde(default)]
    pub couchdb_password: String,
    /// Paths the agent will crawl. Agent does not start if this is empty.
    #[serde(default)]
    pub watch_paths: Vec<String>,
    /// Paths excluded from crawling (optional).
    #[serde(default)]
    pub excluded_paths: Vec<String>,
}

impl Settings {
    pub fn is_configured(&self) -> bool {
        !self.couchdb_url.is_empty()
    }
}

pub fn path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("settings.json")
}

pub fn load(app_data_dir: &Path) -> Settings {
    std::fs::read_to_string(path(app_data_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app_data_dir: &Path, s: &Settings) -> std::io::Result<()> {
    let p = path(app_data_dir);
    std::fs::create_dir_all(p.parent().unwrap())?;
    let json = serde_json::to_string_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(p, json)
}
