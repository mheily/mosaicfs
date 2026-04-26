use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, WebviewWindowBuilder};

mod bookmarks;
mod commands;
mod connection;
#[cfg(target_os = "macos")]
mod macos;
mod server;
mod settings;
#[allow(dead_code)]
mod stub;

fn open_or_focus(app: &AppHandle, label: &str, title: &str, url: &str, w: f64, h: f64) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let _ = WebviewWindowBuilder::new(
            app,
            label,
            tauri::WebviewUrl::External(url.parse().unwrap()),
        )
        .title(title)
        .inner_size(w, h)
        .build();
    }
}

fn open_setup_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("setup") {
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let _ = WebviewWindowBuilder::new(
            app,
            "setup",
            tauri::WebviewUrl::App("setup.html".into()),
        )
        .title("MosaicFS — Database Connection")
        .inner_size(400.0, 340.0)
        .resizable(false)
        .build();
    }
}

fn base_url(app: &AppHandle) -> String {
    let port = app.state::<server::ProxyPort>().0;
    format!("http://127.0.0.1:{port}")
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(app: AppHandle) -> settings::Settings {
    let dir = app.path().app_data_dir().unwrap_or_default();
    settings::load(&dir)
}

/// Probe CouchDB. On success returns the normalized URL (with default port
/// applied) so the frontend can display what will actually be saved.
#[tauri::command]
async fn test_connection(
    couchdb_url: String,
    couchdb_user: String,
    couchdb_password: String,
) -> Result<String, String> {
    connection::test(&couchdb_url, &couchdb_user, &couchdb_password).await
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    couchdb_url: String,
    couchdb_user: String,
    couchdb_password: String,
) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // Re-test (and normalize) at save time so we never persist settings the
    // server can't use. The frontend will normally have already tested, but
    // this catches direct invocations and stale UI state.
    let normalized = connection::test(&couchdb_url, &couchdb_user, &couchdb_password).await?;

    let s = settings::Settings {
        couchdb_url: normalized,
        couchdb_user,
        couchdb_password,
    };
    settings::save(&dir, &s).map_err(|e| e.to_string())?;

    // Build a fresh in-process router and atomically swap it in.
    let router = server::build_router(&s, &dir).await.map_err(|e| e.to_string())?;
    if let Some(slot) = app.try_state::<Arc<server::RouterSlot>>() {
        slot.set(router);
    }

    if let Some(win) = app.get_webview_window("setup") {
        let _ = win.close();
    }

    Ok(())
}

// ── App entry point ───────────────────────────────────────────────────────────

pub fn run() {
    // The mosaicfs-server crate calls into rustls (for TLS cert generation
    // even in plain-HTTP mode). The crypto provider must be installed once,
    // here, before any router is built.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .setup(|app| {
            // ── Bookmarks store ───────────────────────────────────────────
            let store_path = app.path().app_data_dir()?.join("bookmarks.json");
            std::fs::create_dir_all(store_path.parent().unwrap()).ok();
            let store = bookmarks::BookmarkStore::load(store_path);
            app.manage(Mutex::new(store));

            // ── Settings + in-process server ──────────────────────────────
            let app_data_dir = app.path().app_data_dir()?;
            let s = settings::load(&app_data_dir);

            let slot = Arc::new(server::RouterSlot::new());
            let proxy_port = server::start(Arc::clone(&slot))
                .map_err(|e| format!("listener: {e}"))?;
            app.manage(server::ProxyPort(proxy_port));
            app.manage(Arc::clone(&slot));

            if s.is_configured() {
                let dir = app_data_dir.clone();
                let slot = Arc::clone(&slot);
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match server::build_router(&s, &dir).await {
                        Ok(router) => slot.set(router),
                        Err(e) => {
                            eprintln!("mosaicfs-desktop: router build failed: {e}");
                            // Show the setup window so the user can fix the
                            // CouchDB connection. The router slot stays empty
                            // and any open window keeps showing the retry page.
                            open_setup_window(&handle);
                        }
                    }
                });
            } else {
                let handle = app.handle().clone();
                // Defer opening the window until after setup() returns so the
                // tray is already visible when the form appears.
                tauri::async_runtime::spawn(async move {
                    open_setup_window(&handle);
                });
            }

            // ── macOS app menu ────────────────────────────────────────────
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{MenuBuilder, MenuItem, SubmenuBuilder};

                let settings_item = MenuItem::with_id(
                    app,
                    "open_settings",
                    "Settings...",
                    true,
                    Some("cmd+,"),
                )?;

                let app_submenu = SubmenuBuilder::new(app, "MosaicFS")
                    .about(None)
                    .separator()
                    .item(&settings_item)
                    .separator()
                    .services()
                    .separator()
                    .hide()
                    .hide_others()
                    .show_all()
                    .separator()
                    .quit()
                    .build()?;

                let menu = MenuBuilder::new(app).item(&app_submenu).build()?;
                app.set_menu(menu)?;
            }

            // ── System tray ───────────────────────────────────────────────
            {
                use tauri::menu::{MenuBuilder, MenuItem};
                use tauri::tray::TrayIconBuilder;

                let browse_item = MenuItem::with_id(
                    app, "tray_browse", "Browse", true, None::<&str>,
                )?;
                let status_item = MenuItem::with_id(
                    app, "tray_status", "Status", true, None::<&str>,
                )?;
                let settings_item = MenuItem::with_id(
                    app, "tray_settings", "Settings...", true, None::<&str>,
                )?;
                let connection_item = MenuItem::with_id(
                    app, "tray_connection", "Connection...", true, None::<&str>,
                )?;

                let tray_menu = MenuBuilder::new(app)
                    .item(&browse_item)
                    .item(&status_item)
                    .item(&settings_item)
                    .item(&connection_item)
                    .separator()
                    .quit()
                    .build()?;

                TrayIconBuilder::new()
                    .icon(tauri::include_image!("icons/32x32.png"))
                    .menu(&tray_menu)
                    .show_menu_on_left_click(true)
                    .tooltip("MosaicFS")
                    .on_menu_event(|app, event| {
                        let base = base_url(app);
                        match event.id().0.as_str() {
                            "tray_browse" => open_or_focus(
                                app, "main", "MosaicFS",
                                &format!("{base}/ui/browse"), 1200.0, 800.0,
                            ),
                            "tray_status" => open_or_focus(
                                app, "status", "MosaicFS Status",
                                &format!("{base}/ui/status"), 900.0, 600.0,
                            ),
                            "tray_settings" => open_or_focus(
                                app, "admin", "MosaicFS Settings",
                                &format!("{base}/ui/settings/credentials"),
                                1000.0, 700.0,
                            ),
                            "tray_connection" => open_setup_window(app),
                            _ => {}
                        }
                    })
                    .build(app)?;
            }

            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().0 == "open_settings" {
                let base = base_url(app);
                open_or_focus(
                    app, "admin", "MosaicFS Settings",
                    &format!("{base}/ui/settings/credentials"),
                    1000.0, 700.0,
                );
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_file,
            commands::authorize_mount,
            get_settings,
            save_settings,
            test_connection,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
