mod commands;
mod db;
mod models;
mod services;
mod tray;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

use commands::{
    cancel_recording, delete_dictionary_entry, delete_snippet, get_app_snapshot, get_model_inventory,
    get_model_status, get_recording_level, get_shortcut_status, inject_text, list_microphones,
    save_settings, set_shortcuts_paused, start_recording, stop_recording, upsert_dictionary_entry,
    upsert_snippet,
};
use services::{app_state::AppState, shortcuts};

fn install_global_shortcut(
    app: &mut tauri::App,
    shortcut_status: std::sync::Arc<parking_lot::Mutex<models::ShortcutStatus>>,
    shortcuts_paused: std::sync::Arc<parking_lot::Mutex<bool>>,
    initial_hotkey: &str,
) -> anyhow::Result<()> {
    #[cfg(desktop)]
    {
        app.handle().plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if *shortcuts_paused.lock() {
                        return;
                    }
                    let payload = match event.state() {
                        ShortcutState::Pressed => "pressed",
                        ShortcutState::Released => "released",
                    };
                    let _ = app.emit("wind-speak://shortcut", payload);
                })
                .build(),
        )?;
        shortcuts::register_shortcut(app.handle(), shortcut_status, initial_hotkey, false);
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new().expect("failed to initialize application state");
    let shortcut_status = app_state.shortcut_status.clone();
    let shortcuts_paused = app_state.shortcuts_paused.clone();
    let initial_hotkey = app_state
        .database
        .lock()
        .load_settings()
        .map(|settings| settings.hotkey)
        .unwrap_or_else(|_| "Ctrl+Win+Space".to_string());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            install_global_shortcut(
                app,
                shortcut_status.clone(),
                shortcuts_paused.clone(),
                &initial_hotkey,
            )?;
            tray::install(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    "main" => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    "overlay" => {
                        api.prevent_close();
                        let _ = window.hide();
                        if let Some(main) = window.app_handle().get_webview_window("main") {
                            let _ = main.emit(
                                "wind-speak://overlay-visibility",
                                "Floating control hidden. Reopen it from the tray menu.",
                            );
                        }
                    }
                    _ => {}
                }
            }
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_shortcut_status,
            get_recording_level,
            list_microphones,
            save_settings,
            set_shortcuts_paused,
            start_recording,
            stop_recording,
            cancel_recording,
            inject_text,
            upsert_dictionary_entry,
            delete_dictionary_entry,
            upsert_snippet,
            delete_snippet,
            get_model_status,
            get_model_inventory
        ])
        .run(tauri::generate_context!())
        .expect("error while running Wind Speak");
}
