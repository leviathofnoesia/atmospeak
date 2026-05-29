use tauri::{
    Emitter, Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub fn install(app: &mut tauri::App) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open Wind Speak").build(app)?;
    let dictate = MenuItemBuilder::with_id("dictate", "Start / Stop Dictation").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &dictate, &quit])
        .build()?;

    TrayIconBuilder::new()
        .tooltip("Wind Speak")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "dictate" => {
                let _ = app.emit("wind-speak://shortcut", "toggle");
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(&tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
