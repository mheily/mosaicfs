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

/// Returns the Unix socket path. Uses temp_dir so the path stays short —
/// the sandboxed app_data_dir exceeds the 104-byte sockaddr_un limit on macOS.
pub fn socket_path() -> PathBuf {
    std::env::temp_dir().join("mosaicfs.sock")
}

/// Write server.toml from `settings`. Always overwrites so changes take effect
/// on the next server start.
pub fn write_config(app_data_dir: &Path, settings: &Settings) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(app_data_dir)?;
    let config_path = app_data_dir.join("server.toml");
    let data_dir = app_data_dir.join("server-data");
    let socket_path = socket_path();

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
            // Quick connect attempt — absorbs a brief startup race.
            match tokio::net::UnixStream::connect(&sock).await {
                Ok(mut unix_stream) => {
                    let _ =
                        tokio::io::copy_bidirectional(&mut tcp_stream, &mut unix_stream).await;
                }
                Err(_) => {
                    // WKWebView silently blank-pages on non-2xx responses, so we
                    // return 200 with an HTML page that retries via JS every 2 s.
                    // After 30 s it shows the config path so the user knows what
                    // to fix.
                    use tokio::io::AsyncWriteExt;
                    let config_path = sock.with_file_name("server.toml");
                    let config_str = config_path
                        .display()
                        .to_string()
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"");
                    let body = format!(
                        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<title>MosaicFS — Connecting…</title>
<style>
  body{{font-family:-apple-system,system-ui,sans-serif;text-align:center;
       padding:60px 40px;color:#555;}}
  code{{background:#f0f0f0;padding:2px 6px;border-radius:4px;font-size:.85em;
       word-break:break-all;}}
  #err{{display:none;color:#c00;margin-top:20px;}}
</style>
</head><body>
<h2>MosaicFS</h2>
<p id="msg">Connecting to server…</p>
<p id="err">The server did not start.<br>
Check the CouchDB URL in<br><code>{config_str}</code><br>
then choose <strong>Connection…</strong> from the menu bar.</p>
<script>
var t0 = Date.now();
function retry() {{
  if (Date.now() - t0 > 30000) {{
    document.getElementById('msg').style.display = 'none';
    document.getElementById('err').style.display = 'block';
  }} else {{
    location.reload();
  }}
}}
setTimeout(retry, 2000);
</script>
</body></html>"#
                    );
                    let _ = tcp_stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\n\
                                 Content-Type: text/html; charset=utf-8\r\n\
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
