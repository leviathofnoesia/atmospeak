mod commands;
mod db;
mod models;
mod services;
mod tray;

use tauri::{Emitter, Manager, WindowEvent};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_global_shortcut::ShortcutState;

use commands::{
    cancel_recording, delete_dictionary_entry, delete_snippet, get_app_snapshot,
    get_last_stage_metrics, get_model_inventory, get_model_status, get_recording_level,
    get_runtime_events, get_shortcut_status, handle_dictation_action, inject_text, list_microphones,
    mic_check_start, mic_check_stop, save_settings, set_shortcut_test_active, set_shortcuts_paused,
    show_main_window, show_overlay_window, start_recording, stop_recording,
    upsert_dictionary_entry, upsert_snippet,
};
use services::{app_state::AppState, dictation_engine, overlay_window, shortcuts};

/// Must match frontend `ONBOARDING_VERSION` in `src/types/dictation.ts`.
const ONBOARDING_VERSION: &str = "phase-a-honest-mvp-v1";

fn install_global_shortcut(
    app: &mut tauri::App,
    shortcut_status: std::sync::Arc<parking_lot::Mutex<models::ShortcutStatus>>,
    shortcuts_paused: std::sync::Arc<parking_lot::Mutex<bool>>,
    initial_hotkey: &str,
) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        shortcuts::register_shortcut(
            app.handle(),
            shortcut_status,
            shortcuts_paused,
            initial_hotkey,
            false,
        );
    }

    #[cfg(all(desktop, not(target_os = "windows")))]
    {
        let shortcuts_paused_for_registration = shortcuts_paused.clone();
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
                    dictation_engine::route_shortcut_payload(app, payload);
                })
                .build(),
        )?;
        shortcuts::register_shortcut(
            app.handle(),
            shortcut_status,
            shortcuts_paused_for_registration,
            initial_hotkey,
            false,
        );
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
        .unwrap_or_else(|_| "Ctrl+Win".to_string());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .setup(move |app| {
            let engine = dictation_engine::spawn(app.handle().clone());
            app.state::<AppState>().set_engine(engine);

            install_global_shortcut(
                app,
                shortcut_status.clone(),
                shortcuts_paused.clone(),
                &initial_hotkey,
            )?;
            tray::install(app)?;
            let needs_onboarding = app
                .state::<AppState>()
                .database
                .lock()
                .load_settings()
                .map(|settings| {
                    !settings.onboarding_complete
                        || settings.onboarding_version != ONBOARDING_VERSION
                })
                .unwrap_or(true);
            if needs_onboarding {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.unminimize();
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }
            let _ = overlay_window::show_and_reset(app.handle());
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
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_shortcut_status,
            get_recording_level,
            list_microphones,
            save_settings,
            set_shortcuts_paused,
            show_overlay_window,
            show_main_window,
            set_shortcut_test_active,
            get_runtime_events,
            get_last_stage_metrics,
            start_recording,
            stop_recording,
            cancel_recording,
            handle_dictation_action,
            mic_check_start,
            mic_check_stop,
            inject_text,
            upsert_dictionary_entry,
            delete_dictionary_entry,
            upsert_snippet,
            delete_snippet,
            get_model_status,
            get_model_inventory
        ])
        .run(tauri::generate_context!())
        .expect("error while running Atmospeak");
}
