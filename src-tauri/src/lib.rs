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
    clear_polish_api_key, complete_onboarding, delete_dictionary_entry, delete_model,
    delete_session, delete_snippet, download_model, download_polish_model, ensure_polish_runtime,
    finish_sound_check, get_app_snapshot, get_edition, get_last_stage_metrics, get_model_inventory,
    get_model_status, get_polish_inventory, get_runtime_events,
    get_shortcut_status, handle_dictation_action, has_polish_api_key, inject_onboarding_sample,
    copy_transcript, open_history, take_history_request, dismiss_dictation_error, get_dictation_snapshot, search_history, get_history_session,
    inject_session, list_microphones, mic_check_start, mic_check_stop, open_windows_sound_settings,
    polish_session, register_setup_shortcut, reset_overlay_position, save_overlay_position,
    save_settings, set_overlay_hit_regions, set_polish_api_key, set_session_prefer_polished, set_shortcut_test_active,
    set_shortcuts_paused, show_main_window, show_overlay_window, start_recording,
    start_shortcut_capture, start_sound_check, stop_recording, streaming_asr_available,
    upsert_dictionary_entry, upsert_snippet,
};
use services::{
    app_state::AppState, asr_host, dictation_engine, metrics, runtime, shortcuts, streaming_asr,
    window_manager,
};

pub(crate) fn start_preferred_asr(app: &tauri::AppHandle) {
    let generation = app.state::<AppState>().begin_streaming_asr_warmup();
    let app = app.clone();
    std::thread::Builder::new()
        .name("atmospeak-streaming-asr-warmup".into())
        .spawn(move || {
            if !streaming_warmup_is_current(&app, generation) {
                return;
            }
            publish_lazy_asr_host(&app);
            let Ok(settings) = app.state::<AppState>().database.lock().load_settings() else {
                return;
            };
            let Ok(resolved) = runtime::resolve_runtime(&app, &settings) else {
                if !streaming_warmup_is_current(&app, generation) {
                    return;
                }
                metrics::emit_runtime(
                    &app,
                    "streaming-asr-unavailable",
                    "Bundled Parakeet CPU runtime is unavailable.",
                );
                return;
            };
            if !streaming_warmup_is_current(&app, generation) {
                return;
            }
            let threads = std::thread::available_parallelism()
                .map(|threads| threads.get().min(8) as u16)
                .unwrap_or(4);
            let startup_is_current = || streaming_warmup_is_current(&app, generation);
            match streaming_asr::StreamingAsr::spawn(
                app.clone(),
                resolved.executable_path,
                resolved.model_path,
                threads,
                startup_is_current,
            ) {
                Ok(host) => {
                    if !app
                        .state::<AppState>()
                        .set_streaming_asr_if_current(generation, host)
                    {
                        return;
                    }
                    metrics::emit_runtime(
                        &app,
                        "streaming-asr-ready",
                        "Streaming local ASR is ready with CPU.",
                    );
                }
                Err(error) if streaming_asr::is_startup_superseded(&error) => return,
                Err(error) => {
                    if !streaming_warmup_is_current(&app, generation) {
                        return;
                    }
                    metrics::emit_runtime(&app, "streaming-asr-error", error.to_string());
                }
            }
        })
        .expect("failed to spawn streaming ASR warmup thread");
}

fn streaming_warmup_is_current(app: &tauri::AppHandle, generation: u64) -> bool {
    app.state::<AppState>().streaming_asr_is_current(generation)
}

/// Publish the generic CPU host without warming a second model copy. The host
/// validates the shared executable and model directory when first used.
pub(crate) fn publish_lazy_asr_host(app: &tauri::AppHandle) {
    let Ok(settings) = app.state::<AppState>().database.lock().load_settings() else {
        return;
    };
    let Ok(resolved) = runtime::resolve_runtime(app, &settings) else {
        return;
    };
    if let Ok(host) = asr_host::AsrHost::new(resolved.executable_path, resolved.model_path) {
        app.state::<AppState>()
            .set_asr_host(std::sync::Arc::new(host));
    }
}

/// Must match frontend `ONBOARDING_VERSION` in `src/types/dictation.ts`.
pub(crate) const ONBOARDING_VERSION: &str = "atmospeak-setup-v2";

fn install_global_shortcut(
    app: &mut tauri::App,
    shortcut_status: std::sync::Arc<parking_lot::Mutex<models::ShortcutStatus>>,
    shortcuts_paused: std::sync::Arc<parking_lot::Mutex<bool>>,
    initial_hotkey: &str,
    initial_mode: models::DictationMode,
) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        shortcuts::register_shortcut(
            app.handle(),
            shortcut_status,
            shortcuts_paused,
            initial_hotkey,
            initial_mode,
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
            initial_mode,
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
            services::voice_edit::install(app.handle())?;
            let engine = dictation_engine::spawn(app.handle().clone());
            app.state::<AppState>().set_engine(engine);
            start_preferred_asr(app.handle());
            tray::install(app)?;
            if window_manager::setup_is_complete(app.handle(), ONBOARDING_VERSION) {
                let (hotkey, mode) = app
                    .state::<AppState>()
                    .database
                    .lock()
                    .load_settings()
                    .map(|settings| (settings.hotkey, settings.mode))
                    .unwrap_or_else(|_| {
                        ("Ctrl+Win".to_string(), models::DictationMode::PushToTalk)
                    });
                install_global_shortcut(
                    app,
                    shortcut_status.clone(),
                    shortcuts_paused.clone(),
                    &hotkey,
                    mode,
                )?;
                let settings = app.state::<AppState>().database.lock().load_settings()?;
                if let Err(error) =
                    services::voice_edit::register(app.handle(), "", &settings.voice_edit_hotkey)
                {
                    metrics::emit_runtime(
                        app.handle(),
                        "voice-edit-shortcut-error",
                        error.to_string(),
                    );
                }
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
                    "main" => {
                        if let Some(state) = window.app_handle().try_state::<AppState>() {
                            state.clear_shortcut_interaction_state();
                        }
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(invoke_handlers())
        .build(tauri::generate_context!())
        .expect("error while building Atmospeak")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Stop the resident model before we go. The job object is the
                // backstop for crashes; this is the clean path.
                if let Some(state) = app.try_state::<AppState>() {
                    state.shutdown_streaming_asr();
                    state.shutdown_asr_host();
                    state.shutdown_llama_host();
                }
            }
        });
}

fn invoke_handlers() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_edition,
        get_app_snapshot,
        get_shortcut_status,
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
        set_overlay_hit_regions,
        get_runtime_events,
        streaming_asr_available,
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
        inject_onboarding_sample,
        copy_transcript,
        open_history,
        take_history_request,
        get_dictation_snapshot,
        dismiss_dictation_error,
        search_history,
        get_history_session,
        inject_session,
        upsert_dictionary_entry,
        delete_dictionary_entry,
        upsert_snippet,
        delete_snippet,
        delete_session,
        polish_session,
        set_session_prefer_polished,
        get_polish_inventory,
        download_polish_model,
        ensure_polish_runtime,
        set_polish_api_key,
        clear_polish_api_key,
        has_polish_api_key,
        get_model_status,
        get_model_inventory,
        download_model,
        cancel_model_download,
        delete_model
    ]
}
