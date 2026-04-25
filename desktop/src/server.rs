use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use crate::settings::Settings;

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

/// Write server.toml from `settings`. Always overwrites so changes take effect
/// on the next server start.
pub fn write_config(app_data_dir: &Path, settings: &Settings) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(app_data_dir)?;
    let config_path = app_data_dir.join("server.toml");
    let data_dir = app_data_dir.join("server-data");
    let socket_path = app_data_dir.join("server.sock");

    let toml = format!(
        r#"[features]
agent  = false
vfs    = false
web_ui = true

[couchdb]
url      = "{}"
user     = "{}"
password = "{}"

[web_ui]
data_dir    = "{}"
socket_path = "{}"
"#,
        settings.couchdb_url,
        settings.couchdb_user,
        settings.couchdb_password,
        data_dir.to_string_lossy(),
        socket_path.to_string_lossy(),
    );
    std::fs::write(&config_path, toml)?;
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
