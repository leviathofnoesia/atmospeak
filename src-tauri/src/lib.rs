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
    start_sound_check, stop_recording, streaming_asr_available, upsert_dictionary_entry,
    upsert_snippet,
};
use services::{
    app_state::AppState, asr_host, dictation_engine, metrics, runtime, shortcuts, streaming_asr,
    window_manager,
};

fn streaming_disabled() -> bool {
    matches!(
        std::env::var("ATMOSPEAK_STREAMING_ASR").ok().as_deref(),
        Some("0") | Some("false")
    )
}

fn resolve_automatic_model(
    app: &tauri::AppHandle,
    settings: &mut models::AppSettings,
    backend: models::AsrBackend,
) {
    if settings.model_selection_mode != models::ModelSelectionMode::Automatic {
        return;
    }
    let preferred = settings.active_model_id.clone();
    let candidate = app
        .state::<AppState>()
        .database
        .lock()
        .automatic_model_candidate(&preferred, backend)
        .ok()
        .flatten();
    if let Some(candidate) = candidate {
        let installed = runtime::model_inventory(app, settings)
            .models
            .into_iter()
            .any(|model| model.id == candidate && model.installed);
        if installed {
            settings.active_model_id = candidate;
        }
    }
}

pub(crate) fn start_preferred_asr(app: &tauri::AppHandle) {
    if streaming_disabled() {
        metrics::emit_runtime(
            app,
            "streaming-asr-disabled",
            "ATMOSPEAK_STREAMING_ASR=0 — using the resident batch host.",
        );
        start_asr_host(app);
        return;
    }
    let generation = app.state::<AppState>().begin_streaming_asr_warmup();
    let app = app.clone();
    std::thread::Builder::new()
        .name("atmospeak-streaming-asr-warmup".into())
        .spawn(move || {
            // Publish the lazy batch host before the multi-second streaming
            // model load so sound-check and fallbacks do not report
            // backend_unavailable while the sidecar is still warming.
            publish_lazy_asr_host(&app);
            let Ok(mut settings) = app.state::<AppState>().database.lock().load_settings() else {
                start_asr_host(&app);
                return;
            };
            let requested_backend = match std::env::var("ATMOSPEAK_ASR_BACKEND").ok().as_deref() {
                Some("cpu") => models::AsrBackend::Cpu,
                _ if settings.acceleration_preference == models::AccelerationPreference::Cpu => {
                    models::AsrBackend::Cpu
                }
                _ => models::AsrBackend::Vulkan,
            };
            let resolved_sidecar = streaming_asr::resolve_executable(&app, requested_backend)
                .map(|executable| (requested_backend, executable))
                .or_else(|| {
                    (requested_backend == models::AsrBackend::Vulkan)
                        .then(|| {
                            streaming_asr::resolve_executable(&app, models::AsrBackend::Cpu)
                                .map(|executable| (models::AsrBackend::Cpu, executable))
                        })
                        .flatten()
                });
            let Some((backend, executable)) = resolved_sidecar else {
                metrics::emit_runtime(
                    &app,
                    "streaming-asr-unavailable",
                    "Streaming sidecar is not bundled; using the resident batch host.",
                );
                start_asr_host(&app);
                return;
            };
            resolve_automatic_model(&app, &mut settings, backend);
            let Ok(resolved) = runtime::resolve_runtime(&app, &settings) else {
                start_asr_host(&app);
                return;
            };
            let threads = std::thread::available_parallelism()
                .map(|threads| threads.get().min(8) as u16)
                .unwrap_or(4);
            match streaming_asr::StreamingAsr::spawn(
                app.clone(),
                executable,
                resolved.model_path,
                backend,
                threads,
            ) {
                Ok(host) => {
                    if !app
                        .state::<AppState>()
                        .set_streaming_asr_if_current(generation, host)
                    {
                        return;
                    }
                    publish_lazy_asr_host(&app);
                    metrics::emit_runtime(
                        &app,
                        "streaming-asr-ready",
                        format!("Streaming local ASR is ready with {backend:?}."),
                    );
                }
                Err(error) if backend == models::AsrBackend::Vulkan => {
                    metrics::emit_runtime(
                        &app,
                        "streaming-asr-vulkan-fallback",
                        format!("Vulkan unavailable: {error}"),
                    );
                    let Some(cpu_executable) =
                        streaming_asr::resolve_executable(&app, models::AsrBackend::Cpu)
                    else {
                        start_asr_host(&app);
                        return;
                    };
                    resolve_automatic_model(&app, &mut settings, models::AsrBackend::Cpu);
                    let Ok(resolved) = runtime::resolve_runtime(&app, &settings) else {
                        start_asr_host(&app);
                        return;
                    };
                    match streaming_asr::StreamingAsr::spawn(
                        app.clone(),
                        cpu_executable,
                        resolved.model_path,
                        models::AsrBackend::Cpu,
                        threads,
                    ) {
                        Ok(host) => {
                            if !app
                                .state::<AppState>()
                                .set_streaming_asr_if_current(generation, host)
                            {
                                return;
                            }
                            publish_lazy_asr_host(&app);
                            metrics::emit_runtime(
                                &app,
                                "streaming-asr-ready",
                                "Streaming local ASR is ready with CPU fallback.",
                            );
                        }
                        Err(error) => {
                            metrics::emit_runtime(&app, "streaming-asr-error", error.to_string());
                            start_asr_host(&app);
                        }
                    }
                }
                Err(error) => {
                    metrics::emit_runtime(&app, "streaming-asr-error", error.to_string());
                    start_asr_host(&app);
                }
            }
        })
        .expect("failed to spawn streaming ASR warmup thread");
}

/// Bring up the resident ASR host in the background: loading the model takes
/// seconds and must not delay the window. Dictation works off the CLI backend
/// until the host is warm, and keeps working if it never comes up at all.
/// Publish the resident batch host in its lazy form alongside streaming: the
/// `whisper-server` process spawns on first use and then stays resident (v0.4
/// behavior). Without this, a streaming fallback pays a cold one-shot
/// `whisper-cli` model load over the entire recording.
fn publish_lazy_asr_host(app: &tauri::AppHandle) {
    if asr_host::is_disabled() {
        return;
    }
    let Ok(mut settings) = app.state::<AppState>().database.lock().load_settings() else {
        return;
    };
    resolve_automatic_model(app, &mut settings, models::AsrBackend::Host);
    let Some(server_exe) = runtime::resolve_server(app, &settings) else {
        return;
    };
    let Ok(resolved) = runtime::resolve_runtime(app, &settings) else {
        return;
    };
    if let Ok(host) = asr_host::AsrHost::new(server_exe, resolved.model_path) {
        app.state::<AppState>().set_asr_host(std::sync::Arc::new(host));
    }
}

/// Bring up the resident ASR host in the background: loading the model takes
/// seconds and must not delay the window. Dictation works off the CLI backend
/// until the host is warm, and keeps working if it never comes up at all.
fn start_asr_host(app: &tauri::AppHandle) {
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
            let Ok(mut settings) = app.state::<AppState>().database.lock().load_settings() else {
                return;
            };
            resolve_automatic_model(&app, &mut settings, models::AsrBackend::Host);
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
                    state.shutdown_streaming_asr();
                    state.shutdown_asr_host();
                }
            }
        });
}
