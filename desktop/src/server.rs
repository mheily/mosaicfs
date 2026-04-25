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

/// Walk up the directory tree from the executable looking for
/// dev-config/mosaicfs.toml and extract the [couchdb] url from it.
///
/// Works for both dev builds (target/debug/…) and bundled releases
/// (MosaicFS.app/Contents/MacOS/…) as long as the app is run from
/// within the project directory.
fn detect_dev_couchdb_url() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    for _ in 0..12 {
        let candidate = dir.join("dev-config/mosaicfs.toml");
        if let Ok(content) = std::fs::read_to_string(&candidate) {
            if let Some(url) = parse_couchdb_url(&content) {
                return Some(url);
            }
        }
        dir = match dir.parent() {
            Some(p) => p,
            None => break,
        };
    }
    None
}

fn parse_couchdb_url(toml: &str) -> Option<String> {
    let mut in_couchdb = false;
    for line in toml.lines() {
        let t = line.trim();
        if t == "[couchdb]" {
            in_couchdb = true;
            continue;
        }
        if t.starts_with('[') {
            in_couchdb = false;
        }
        if in_couchdb && t.starts_with("url") {
            if let Some(val) = t.splitn(2, '=').nth(1) {
                let url = val.trim().trim_matches('"').to_string();
                if !url.is_empty() {
                    return Some(url);
                }
            }
        }
    }
    None
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

        // Use the dev environment's CouchDB URL when available; fall back to
        // localhost for standalone / production installs.
        let couchdb_url =
            detect_dev_couchdb_url().unwrap_or_else(|| "http://localhost:5984".to_string());

        let toml = format!(
            r#"[features]
agent  = false
vfs    = false
web_ui = true

[couchdb]
url      = "{couchdb_url}"
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
            match connect_with_retry(&sock).await {
                Some(mut unix_stream) => {
                    let _ =
                        tokio::io::copy_bidirectional(&mut tcp_stream, &mut unix_stream).await;
                }
                None => {
                    // Return a real HTTP response so the browser shows an error
                    // page rather than a blank white screen.
                    use tokio::io::AsyncWriteExt;
                    let config_path = sock.with_file_name("server.toml");
                    let body = format!(
                        "MosaicFS server is not running.\n\n\
                         The server failed to start or could not connect to CouchDB.\n\
                         Check the CouchDB URL in:\n  {}\n\n\
                         Then restart the app.",
                        config_path.display()
                    );
                    let _ = tcp_stream
                        .write_all(
                            format!(
                                "HTTP/1.1 503 Service Unavailable\r\n\
                                 Content-Type: text/plain; charset=utf-8\r\n\
                                 Content-Length: {}\r\n\
                                 Connection: close\r\n\
                                 \r\n{}",
                                body.len(),
                                body
                            )
                            .as_bytes(),
                        )
                        .await;
                }
            }
        });
    }
}

/// Retry connecting to the Unix socket for up to 10 s to absorb server startup lag.
#[cfg(unix)]
async fn connect_with_retry(sock: &Path) -> Option<tokio::net::UnixStream> {
    for _ in 0..100 {
        match tokio::net::UnixStream::connect(sock).await {
            Ok(s) => return Some(s),
            Err(_) => {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }
    None
}
