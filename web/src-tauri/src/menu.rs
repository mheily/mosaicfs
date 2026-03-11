use tauri::{
    menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    App, Emitter,
};

pub fn setup_menu(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle();

    // App menu (macOS only — other platforms ignore this)
    let about = PredefinedMenuItem::about(handle, Some("About MosaicFS"), None)?;
    let separator = PredefinedMenuItem::separator(handle)?;
    let quit = PredefinedMenuItem::quit(handle, Some("Quit MosaicFS"))?;
    let app_menu = SubmenuBuilder::new(handle, "MosaicFS")
        .item(&about)
        .item(&separator)
        .item(&quit)
        .build()?;

    // File menu
    let close_window = PredefinedMenuItem::close_window(handle, Some("Close Window"))?;
    let file_menu = SubmenuBuilder::new(handle, "File")
        .item(&close_window)
        .build()?;

    // Edit menu
    let undo = PredefinedMenuItem::undo(handle, None)?;
    let redo = PredefinedMenuItem::redo(handle, None)?;
    let sep2 = PredefinedMenuItem::separator(handle)?;
    let cut = PredefinedMenuItem::cut(handle, None)?;
    let copy = PredefinedMenuItem::copy(handle, None)?;
    let paste = PredefinedMenuItem::paste(handle, None)?;
    let select_all = PredefinedMenuItem::select_all(handle, None)?;
    let edit_menu = SubmenuBuilder::new(handle, "Edit")
        .item(&undo)
        .item(&redo)
        .item(&sep2)
        .item(&cut)
        .item(&copy)
        .item(&paste)
        .item(&select_all)
        .build()?;

    // Go menu
    let go_back = MenuItemBuilder::with_id("go-back", "Back")
        .accelerator("CmdOrCtrl+[")
        .build(handle)?;
    let go_forward = MenuItemBuilder::with_id("go-forward", "Forward")
        .accelerator("CmdOrCtrl+]")
        .build(handle)?;
    let go_enclosing = MenuItemBuilder::with_id("go-enclosing", "Enclosing Folder")
        .accelerator("CmdOrCtrl+Up")
        .build(handle)?;
    let go_menu = SubmenuBuilder::new(handle, "Go")
        .item(&go_back)
        .item(&go_forward)
        .item(&go_enclosing)
        .build()?;

    // Window menu
    let minimize = PredefinedMenuItem::minimize(handle, None)?;
    let maximize = PredefinedMenuItem::maximize(handle, None)?;
    let fullscreen = PredefinedMenuItem::fullscreen(handle, None)?;
    let window_menu = SubmenuBuilder::new(handle, "Window")
        .item(&minimize)
        .item(&maximize)
        .item(&fullscreen)
        .build()?;

    let menu = Menu::with_items(
        handle,
        &[&app_menu, &file_menu, &edit_menu, &go_menu, &window_menu],
    )?;

    app.set_menu(menu)?;

    // Listen for custom menu item clicks and emit to frontend
    let handle_clone = handle.clone();
    app.on_menu_event(move |_app, event| {
        let id = event.id().0.as_str();
        // Only emit for our custom items (go-back, go-forward, go-enclosing)
        if id.starts_with("go-") {
            let _ = handle_clone.emit("menu-action", id);
        }
    });

    Ok(())
}
