use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewWindowBuilder};

mod bookmarks;
mod commands;
#[cfg(target_os = "macos")]
mod macos;
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

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let store_path = app.path().app_data_dir()?.join("bookmarks.json");
            std::fs::create_dir_all(store_path.parent().unwrap()).ok();
            let store = bookmarks::BookmarkStore::load(store_path);
            app.manage(Mutex::new(store));

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

                let tray_menu = MenuBuilder::new(app)
                    .item(&browse_item)
                    .item(&status_item)
                    .item(&settings_item)
                    .separator()
                    .quit()
                    .build()?;

                TrayIconBuilder::new()
                    .icon(tauri::include_image!("icons/32x32.png"))
                    .menu(&tray_menu)
                    .show_menu_on_left_click(true)
                    .tooltip("MosaicFS")
                    .on_menu_event(|app, event| match event.id().0.as_str() {
                        "tray_browse" => open_or_focus(
                            app, "main", "MosaicFS",
                            "http://localhost:8443/ui/browse", 1200.0, 800.0,
                        ),
                        "tray_status" => open_or_focus(
                            app, "status", "MosaicFS Status",
                            "http://localhost:8443/ui/status", 900.0, 600.0,
                        ),
                        "tray_settings" => open_or_focus(
                            app, "admin", "MosaicFS Settings",
                            "http://localhost:8443/ui/settings/credentials",
                            1000.0, 700.0,
                        ),
                        _ => {}
                    })
                    .build(app)?;
            }

            Ok(())
        })
        .on_menu_event(|app, event| {
            // Handles events from the macOS app menu bar (e.g. Cmd+,)
            if event.id().0 == "open_settings" {
                open_or_focus(
                    app, "admin", "MosaicFS Settings",
                    "http://localhost:8443/ui/settings/credentials",
                    1000.0, 700.0,
                );
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_file,
            commands::authorize_mount,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            // Keep the process alive when all windows are closed; the tray is the app.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
