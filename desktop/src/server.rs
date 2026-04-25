use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Tauri managed state holding the server child process.
pub struct ServerProcess(pub Mutex<Option<Child>>);

/// Tauri managed state: the localhost port the proxy listens on.
pub struct ProxyPort(pub u16);

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
        let socket_path = app_data_dir.join("server.sock");
        let data_dir_str = data_dir.to_string_lossy();
        let socket_path_str = socket_path.to_string_lossy();
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
data_dir    = "{data_dir_str}"
socket_path = "{socket_path_str}"
"#
        );
        std::fs::write(&config_path, toml)?;
    }
    Ok(config_path)
}

/// Returns the Unix socket path the server will bind on.
pub fn socket_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("server.sock")
}

/// Spawn the server process. Returns the `Child` handle.
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

/// Bind a random localhost TCP port and start an async task that forwards
/// every connection to the Unix socket at `sock`. Returns the port.
#[cfg(unix)]
pub fn start_proxy(sock: PathBuf) -> std::io::Result<u16> {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = std_listener.local_addr()?.port();
    std_listener.set_nonblocking(true)?;

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mosaicfs-desktop: proxy listener error: {e}");
                return;
            }
        };
        run_proxy(listener, sock).await;
    });

    Ok(port)
}

#[cfg(unix)]
async fn run_proxy(listener: tokio::net::TcpListener, sock: PathBuf) {
    loop {
        let Ok((mut tcp_stream, _)) = listener.accept().await else {
            break;
        };
        let sock = sock.clone();
        tokio::spawn(async move {
            let Ok(mut unix_stream) = tokio::net::UnixStream::connect(&sock).await else {
                return;
            };
            let _ = tokio::io::copy_bidirectional(&mut tcp_stream, &mut unix_stream).await;
        });
    }
}
