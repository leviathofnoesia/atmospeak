mod commands;
mod db;
mod models;
mod services;
mod tray;

use tauri::{Emitter, Manager, WindowEvent};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_global_shortcut::ShortcutState;

use commands::{
    cancel_model_download, cancel_recording, cancel_shortcut_capture, cancel_sound_check,
    complete_onboarding, delete_dictionary_entry, delete_model, delete_session, delete_snippet,
    download_model, finish_sound_check, get_app_snapshot, get_last_stage_metrics,
    get_model_inventory, get_model_status, get_recording_level, get_runtime_events,
    get_shortcut_status, handle_dictation_action, inject_text, list_microphones, mic_check_start,
    mic_check_stop, open_windows_sound_settings, register_setup_shortcut, reset_overlay_position,
    save_overlay_position, save_settings, set_shortcut_test_active, set_shortcuts_paused,
    show_main_window, show_overlay_window, start_recording, start_shortcut_capture,
    start_sound_check, stop_recording, upsert_dictionary_entry, upsert_snippet,
};
use services::{
    app_state::AppState, asr_host, dictation_engine, metrics, runtime, shortcuts, window_manager,
};

/// Bring up the resident ASR host in the background: loading the model takes
/// seconds and must not delay the window. Dictation works off the CLI backend
/// until the host is warm, and keeps working if it never comes up at all.
pub(crate) fn start_asr_host(app: &tauri::AppHandle) {
    if asr_host::is_disabled() {
        metrics::emit_runtime(
            app,
            "asr-host-disabled",
            "ATMOSPEAK_WHISPER_HOST=0 — using the one-shot CLI backend.",
        );
        return;
    }

    let app = app.clone();
    std::thread::Builder::new()
        .name("atmospeak-asr-host-warmup".into())
        .spawn(move || {
            let Ok(settings) = app.state::<AppState>().database.lock().load_settings() else {
                return;
            };
            let Some(server_exe) = runtime::resolve_server(&app, &settings) else {
                metrics::emit_runtime(
                    &app,
                    "asr-host-unavailable",
                    "whisper-server.exe is not bundled — using the one-shot CLI backend.",
                );
                return;
            };
            let Ok(resolved) = runtime::resolve_runtime(&app, &settings) else {
                return;
            };

            let host = match asr_host::AsrHost::new(server_exe, resolved.model_path) {
                Ok(host) => std::sync::Arc::new(host),
                Err(error) => {
                    metrics::emit_runtime(&app, "asr-host-error", error.to_string());
                    return;
                }
            };

            // Publish the host before warming it. A sound check started during
            // model load can then wait on this same host instead of incorrectly
            // reporting that no resident backend exists (or spawning a second
            // process).
            app.state::<AppState>().set_asr_host(host.clone());
            match host.ensure_running() {
                Ok(_) => {
                    metrics::emit_runtime(&app, "asr-host-ready", "Resident speech model is warm.");
                }
                Err(error) => {
                    app.state::<AppState>().shutdown_asr_host();
                    metrics::emit_runtime(
                        &app,
                        "asr-host-error",
                        format!("resident host unavailable, using CLI backend: {error}"),
                    );
                }
            }
        })
        .expect("failed to spawn ASR host warmup thread");
}

/// Must match frontend `ONBOARDING_VERSION` in `src/types/dictation.ts`.
pub(crate) const ONBOARDING_VERSION: &str = "atmospeak-setup-v2";

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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .setup(move |app| {
            let engine = dictation_engine::spawn(app.handle().clone());
            app.state::<AppState>().set_engine(engine);
            start_asr_host(app.handle());
            tray::install(app)?;
            if window_manager::setup_is_complete(app.handle(), ONBOARDING_VERSION) {
                let hotkey = app
                    .state::<AppState>()
                    .database
                    .lock()
                    .load_settings()
                    .map(|settings| settings.hotkey)
                    .unwrap_or_else(|_| "Ctrl+Win".to_string());
                install_global_shortcut(
                    app,
                    shortcut_status.clone(),
                    shortcuts_paused.clone(),
                    &hotkey,
                )?;
                window_manager::ensure_overlay(app.handle())?;
                let _ = services::overlay_window::show(app.handle());
            } else {
                *shortcuts_paused.lock() = true;
                window_manager::ensure_main(app.handle(), true)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
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
            register_setup_shortcut,
            start_shortcut_capture,
            cancel_shortcut_capture,
            save_overlay_position,
            get_runtime_events,
            get_last_stage_metrics,
            start_recording,
            stop_recording,
            cancel_recording,
            handle_dictation_action,
            mic_check_start,
            mic_check_stop,
            start_sound_check,
            finish_sound_check,
            cancel_sound_check,
            open_windows_sound_settings,
            complete_onboarding,
            reset_overlay_position,
            inject_text,
            upsert_dictionary_entry,
            delete_dictionary_entry,
            upsert_snippet,
            delete_snippet,
            delete_session,
            get_model_status,
            get_model_inventory,
            download_model,
            cancel_model_download,
            delete_model
        ])
        .build(tauri::generate_context!())
        .expect("error while building Atmospeak")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Stop the resident model before we go. The job object is the
                // backstop for crashes; this is the clean path.
                if let Some(state) = app.try_state::<AppState>() {
                    state.shutdown_asr_host();
                }
            }
        });
}
