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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_file,
            commands::authorize_mount,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
