use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Tauri managed state holding the server child process.
pub struct ServerProcess(pub Mutex<Option<Child>>);

/// Find the `mosaicfs` server binary. In dev and in production bundles it
/// lives in the same directory as this executable.
pub fn find_binary() -> PathBuf {
    let exe = std::env::current_exe().expect("cannot locate own executable");
    exe.parent()
        .expect("executable has no parent directory")
        .join("mosaicfs")
}

/// Write a default server.toml into `app_data_dir` if one does not already
/// exist. Returns the path to the config file.
pub fn ensure_config(app_data_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(app_data_dir)?;
    let config_path = app_data_dir.join("server.toml");
    if !config_path.exists() {
        let data_dir = app_data_dir.join("server-data");
        let data_dir_str = data_dir.to_string_lossy();
        let toml = format!(
            r#"[features]
agent  = false
vfs    = false
web_ui = true

[couchdb]
url      = "http://localhost:5984"
user     = "admin"
password = "changeme"

[web_ui]
listen        = "127.0.0.1:8443"
insecure_http = true
data_dir      = "{data_dir_str}"
"#
        );
        std::fs::write(&config_path, toml)?;
    }
    Ok(config_path)
}

/// Spawn the server process. Returns the `Child` handle; the caller is
/// responsible for killing it on exit.
pub fn launch(config_path: &Path) -> std::io::Result<Child> {
    let binary = find_binary();
    Command::new(&binary)
        .arg("--config")
        .arg(config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}
