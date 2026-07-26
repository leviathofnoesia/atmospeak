use tauri::{
    Emitter, Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::services::{
    app_state::AppState, dictation_engine, overlay_window, shortcuts, window_manager,
};

pub fn install(app: &mut tauri::App) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open Atmospeak").build(app)?;
    let overlay = MenuItemBuilder::with_id("overlay", "Show / Hide Floating Control").build(app)?;
    let recenter = MenuItemBuilder::with_id("recenter", "Reset Dock Position").build(app)?;
    let pause = MenuItemBuilder::with_id("pause", "Pause / Resume Shortcuts").build(app)?;
    let dictate = MenuItemBuilder::with_id("dictate", "Start / Stop Dictation").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &overlay, &recenter, &pause, &dictate, &quit])
        .build()?;

    TrayIconBuilder::new()
        .tooltip("Atmospeak")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "overlay" => toggle_overlay_window(app),
            "recenter" => {
                if window_manager::setup_is_complete(app, crate::ONBOARDING_VERSION) {
                    let _ = window_manager::ensure_overlay(app);
                    let _ = overlay_window::show_and_reset(app);
                } else {
                    let _ = window_manager::ensure_main(app, true);
                }
            }
            "pause" => toggle_shortcuts(app),
            "dictate" => {
                let _ = app.emit("wind-speak://shortcut", "toggle");
                dictation_engine::route_shortcut_payload(app, "toggle");
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

fn toggle_shortcuts(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let paused = !*state.shortcuts_paused.lock();
    shortcuts::set_paused(
        app,
        state.shortcut_status.clone(),
        state.shortcuts_paused.clone(),
        paused,
    );
}

fn show_main_window(app: &tauri::AppHandle) {
    let setup = !window_manager::setup_is_complete(app, crate::ONBOARDING_VERSION);
    let _ = window_manager::ensure_main(app, setup);
}

fn toggle_overlay_window(app: &tauri::AppHandle) {
    if !window_manager::setup_is_complete(app, crate::ONBOARDING_VERSION) {
        let _ = window_manager::ensure_main(app, true);
        return;
    }
    if let Some(window) = app.get_webview_window("overlay") {
        let is_visible = window.is_visible().unwrap_or(false);
        if is_visible {
            let _ = window.hide();
        } else {
            // Restore it where the user left it. Re-centring is the explicit
            // "Reset Dock Position" action, not a side effect of unhiding.
            let _ = overlay_window::show(app);
        }
    } else if window_manager::ensure_overlay(app).is_ok() {
        let _ = overlay_window::show(app);
    }
}
