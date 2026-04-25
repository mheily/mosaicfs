use std::sync::Mutex;

use tauri::{Manager, WebviewWindowBuilder};

mod bookmarks;
mod commands;
#[cfg(target_os = "macos")]
mod macos;
#[allow(dead_code)]
mod stub;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let store_path = app.path().app_data_dir()?.join("bookmarks.json");
            std::fs::create_dir_all(store_path.parent().unwrap()).ok();
            let store = bookmarks::BookmarkStore::load(store_path);
            app.manage(Mutex::new(store));
            WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(
                    "http://localhost:8443/ui/browse".parse().unwrap(),
                ),
            )
            .title("MosaicFS")
            .inner_size(1200.0, 800.0)
            .build()?;

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

            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().0 == "open_settings" {
                if let Some(win) = app.get_webview_window("admin") {
                    let _ = win.show();
                    let _ = win.set_focus();
                } else {
                    let _ = WebviewWindowBuilder::new(
                        app,
                        "admin",
                        tauri::WebviewUrl::External(
                            "http://localhost:8443/ui/settings/credentials"
                                .parse()
                                .unwrap(),
                        ),
                    )
                    .title("MosaicFS Settings")
                    .inner_size(1000.0, 700.0)
                    .build();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_file,
            commands::authorize_mount,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
