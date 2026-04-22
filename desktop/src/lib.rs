pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
