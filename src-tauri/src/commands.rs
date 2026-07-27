use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    models::{
        AppSettings, AppSnapshot, DictationResult, DictionaryEntry, InjectionResult,
        MicrophoneInfo, ModelInventory, ModelStatus, RecordingStarted, RuntimeEvent,
        ShortcutStatus, Snippet, SoundCheckResult, StageMetrics,
    },
    services::{
        app_state::AppState,
        dictation_engine::{self, EngineAction},
        injection, model_downloader, overlay_window, proc, runtime, shortcuts, sound_check,
        startup, window_manager,
    },
};

type CommandResult<T> = std::result::Result<T, String>;

fn engine(state: &AppState) -> CommandResult<dictation_engine::EngineHandle> {
    state
        .engine()
        .ok_or_else(|| "dictation engine is not ready".to_string())
}

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshot> {
    state.database.lock().snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_shortcut_status(state: State<'_, AppState>) -> CommandResult<ShortcutStatus> {
    Ok(state.shortcut_status.lock().clone())
}

#[tauri::command]
pub fn get_recording_level(state: State<'_, AppState>) -> f32 {
    state.recorder.level()
}

#[tauri::command]
pub fn list_microphones(state: State<'_, AppState>) -> CommandResult<Vec<MicrophoneInfo>> {
    let selected = state
        .database
        .lock()
        .load_settings()
        .ok()
        .and_then(|settings| settings.microphone_name);
    state
        .recorder
        .list_microphones(selected.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: AppSettings) -> CommandResult<AppSnapshot> {
    tauri::async_runtime::spawn_blocking(move || save_settings_blocking(app, settings))
        .await
        .map_err(|error| error.to_string())?
}

fn save_settings_blocking(app: AppHandle, settings: AppSettings) -> CommandResult<AppSnapshot> {
    let state = app.state::<AppState>();
    let previous_settings = state
        .database
        .lock()
        .load_settings()
        .map_err(|e| e.to_string())?;
    if settings.onboarding_complete
        && settings.onboarding_version == crate::ONBOARDING_VERSION
        && previous_settings.audio_calibration.is_none()
    {
        return Err(
            "A successful host-backed sound check is required before setup can complete."
                .to_string(),
        );
    }
    if previous_settings.audio_calibration.is_some()
        && settings.audio_calibration.is_none()
        && settings.onboarding_complete
    {
        return Err("Audio calibration cannot be cleared while setup is complete.".to_string());
    }
    if settings.onboarding_complete
        && settings
            .audio_calibration
            .as_ref()
            .is_some_and(|calibration| {
                settings.microphone_name.as_deref() != Some(calibration.device_name.as_str())
            })
    {
        return Err(
            "The selected microphone has not been calibrated. Run the diagnostic sound check before saving it."
                .to_string(),
        );
    }
    let runtime_changed = previous_settings.active_model_id != settings.active_model_id
        || previous_settings.advanced_runtime_enabled != settings.advanced_runtime_enabled
        || previous_settings.advanced_model_path != settings.advanced_model_path
        || previous_settings.advanced_whisper_cli_path != settings.advanced_whisper_cli_path;
    let setup_complete = settings.onboarding_complete
        && settings.onboarding_version == crate::ONBOARDING_VERSION
        && settings
            .audio_calibration
            .as_ref()
            .is_some_and(|calibration| calibration.asr_backend == "host");
    if setup_complete {
        startup::set_start_at_login(settings.start_at_login).map_err(|e| e.to_string())?;
    }
    let database = state.database.lock();
    database
        .save_settings(&settings)
        .map_err(|e| e.to_string())?;
    let expired_audio = database
        .prune_sessions(settings.transcript_retention_days)
        .map_err(|e| e.to_string())?;
    let _ = runtime::model_status(&app, &settings);
    if setup_complete {
        shortcuts::register_shortcut(
            &app,
            state.shortcut_status.clone(),
            state.shortcuts_paused.clone(),
            &settings.hotkey,
            *state.shortcuts_paused.lock(),
        );
    }
    // The overlay lives in its own webview and reads settings once on mount, so
    // appearance and hotkey changes have to be pushed to it.
    let _ = app.emit("wind-speak://settings-changed", settings.clone());
    let _ = app.emit("atmospeak://settings-changed", settings.clone());
    let snapshot = database.snapshot().map_err(|e| e.to_string())?;
    drop(database);
    remove_managed_recordings(&state.app_dir, expired_audio);
    if runtime_changed {
        state.shutdown_asr_host();
        crate::start_asr_host(&app);
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn set_shortcuts_paused(
    app: AppHandle,
    state: State<'_, AppState>,
    paused: bool,
) -> ShortcutStatus {
    shortcuts::set_paused(
        &app,
        state.shortcut_status.clone(),
        state.shortcuts_paused.clone(),
        paused,
    )
}

#[tauri::command]
pub async fn show_overlay_window(app: AppHandle) -> CommandResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        window_manager::show_overlay(&app, crate::ONBOARDING_VERSION)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn show_main_window(app: AppHandle) -> CommandResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let setup = !window_manager::setup_is_complete(&app, crate::ONBOARDING_VERSION);
        window_manager::ensure_main(&app, setup).map(|_| ())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_overlay_position(app: AppHandle, x: i32, y: i32) {
    overlay_window::save_position(&app, x, y);
}

#[tauri::command]
pub fn set_shortcut_test_active(app: AppHandle, state: State<'_, AppState>, active: bool) {
    state.set_shortcut_test_active(active);
    if !active && !window_manager::setup_is_complete(&app, crate::ONBOARDING_VERSION) {
        shortcuts::set_paused(
            &app,
            state.shortcut_status.clone(),
            state.shortcuts_paused.clone(),
            true,
        );
    }
}

#[tauri::command]
pub fn register_setup_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkey: String,
) -> ShortcutStatus {
    state.set_shortcut_capture_active(false);
    state.set_shortcut_test_active(true);
    // Setup must not install or replace the global keyboard hook. The hook is
    // registered exactly once by `complete_onboarding`, after calibration has
    // passed. During setup the focused WebView verifies the physical chord.
    let status = shortcuts::validate_shortcut(&hotkey, true);
    *state.shortcut_status.lock() = status.clone();
    let _ = app.emit("wind-speak://shortcut-status", status.clone());
    status
}

#[tauri::command]
pub fn start_shortcut_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    current_hotkey: String,
) -> ShortcutStatus {
    state.set_shortcut_test_active(false);
    state.set_shortcut_capture_active(false);
    let status = shortcuts::register_shortcut(
        &app,
        state.shortcut_status.clone(),
        state.shortcuts_paused.clone(),
        &current_hotkey,
        false,
    );
    state.set_shortcut_capture_active(status.registered);
    status
}

#[tauri::command]
pub fn cancel_shortcut_capture(app: AppHandle, state: State<'_, AppState>) {
    state.set_shortcut_capture_active(false);
    if !window_manager::setup_is_complete(&app, crate::ONBOARDING_VERSION) {
        shortcuts::set_paused(
            &app,
            state.shortcut_status.clone(),
            state.shortcuts_paused.clone(),
            true,
        );
    }
}

#[tauri::command]
pub fn get_runtime_events(state: State<'_, AppState>) -> Vec<RuntimeEvent> {
    state.recent_events(100)
}

#[tauri::command]
pub fn get_last_stage_metrics(state: State<'_, AppState>) -> Option<StageMetrics> {
    state.last_metrics()
}

#[tauri::command]
pub fn start_recording(state: State<'_, AppState>) -> CommandResult<RecordingStarted> {
    engine(&state)?.start_blocking()
}

#[tauri::command]
pub fn stop_recording(state: State<'_, AppState>) -> CommandResult<DictationResult> {
    engine(&state)?.stop_blocking()
}

#[tauri::command]
pub fn cancel_recording(state: State<'_, AppState>) -> CommandResult<()> {
    engine(&state)?.cancel_blocking()
}

#[tauri::command]
pub fn handle_dictation_action(
    state: State<'_, AppState>,
    action: String,
) -> CommandResult<String> {
    let engine = engine(&state)?;
    // "pressed"/"released" are hotkey-shaped edges and must go through the mode-aware
    // arms, so the overlay behaves identically to the hook (D10). "start"/"stop" stay
    // explicit and mode-independent for direct UI buttons.
    let engine_action = match action.as_str() {
        "pressed" => EngineAction::Pressed,
        "released" => EngineAction::Released,
        "start" => EngineAction::Start,
        "stop" => EngineAction::Stop,
        "toggle" => EngineAction::Toggle,
        "cancel" => EngineAction::Cancel,
        other => return Err(format!("unknown dictation action: {other}")),
    };
    if engine.dispatch_fire_and_forget(engine_action) {
        Ok("accepted".to_string())
    } else {
        Err("dictation engine is not running".to_string())
    }
}

#[tauri::command]
pub fn mic_check_start(state: State<'_, AppState>) -> CommandResult<()> {
    engine(&state)?.mic_check_start()
}

#[tauri::command]
pub fn mic_check_stop(state: State<'_, AppState>) -> CommandResult<()> {
    engine(&state)?.mic_check_stop()
}

#[tauri::command]
pub async fn start_sound_check(app: AppHandle, device_name: String) -> CommandResult<()> {
    tauri::async_runtime::spawn_blocking(move || sound_check::start(&app, device_name))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn finish_sound_check(
    app: AppHandle,
    expected_phrase: String,
) -> CommandResult<SoundCheckResult> {
    tauri::async_runtime::spawn_blocking(move || sound_check::finish(&app, expected_phrase))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_sound_check(app: AppHandle) -> bool {
    sound_check::cancel(&app)
}

#[tauri::command]
pub fn open_windows_sound_settings() -> CommandResult<()> {
    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg("ms-settings:sound");
        proc::hide_console(&mut command);
        command.spawn().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn complete_onboarding(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> CommandResult<AppSnapshot> {
    let persisted = state
        .database
        .lock()
        .load_settings()
        .map_err(|error| error.to_string())?;
    let previous_start_at_login = persisted.start_at_login;
    let calibration = persisted
        .audio_calibration
        .filter(|calibration| calibration.asr_backend == "host")
        .ok_or_else(|| {
            "Complete the host-backed sound check before entering Atmospeak.".to_string()
        })?;
    if settings.microphone_name.as_deref() != Some(calibration.device_name.as_str()) {
        return Err(
            "The selected microphone changed after calibration. Run the sound check again."
                .to_string(),
        );
    }
    if settings.active_model_id != calibration.model_id {
        return Err(
            "The selected voice model changed after calibration. Run the sound check again."
                .to_string(),
        );
    }
    settings.audio_calibration = Some(calibration);
    settings.onboarding_complete = true;
    settings.onboarding_version = crate::ONBOARDING_VERSION.to_string();
    let shortcut_app = app.clone();
    let shortcut_status = state.shortcut_status.clone();
    let shortcuts_paused = state.shortcuts_paused.clone();
    let hotkey = settings.hotkey.clone();
    let start_at_login = settings.start_at_login;
    let shortcut = tauri::async_runtime::spawn_blocking(move || {
        let shortcut = shortcuts::register_shortcut(
            &shortcut_app,
            shortcut_status.clone(),
            shortcuts_paused.clone(),
            &hotkey,
            false,
        );
        if !shortcut.registered {
            return Ok::<_, String>(shortcut);
        }
        if let Err(error) = startup::set_start_at_login(start_at_login) {
            let _ = startup::set_start_at_login(previous_start_at_login);
            shortcuts::set_paused(&shortcut_app, shortcut_status, shortcuts_paused, true);
            return Err(error.to_string());
        }
        Ok(shortcut)
    })
    .await
    .map_err(|error| error.to_string())??;
    if !shortcut.registered {
        return Err(shortcut.message);
    }
    state
        .database
        .lock()
        .save_settings(&settings)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("atmospeak://settings-changed", settings.clone());
    let _ = app.emit("wind-speak://settings-changed", settings);

    let window_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || window_manager::finish_setup(&window_app))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;

    state
        .database
        .lock()
        .snapshot()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reset_overlay_position(app: AppHandle) -> CommandResult<()> {
    overlay_window::show_and_reset(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn inject_text(state: State<'_, AppState>, text: String) -> CommandResult<InjectionResult> {
    let settings = state
        .database
        .lock()
        .load_settings()
        .map_err(|e| e.to_string())?;
    let preferred = state
        .last_target_window()
        .map(|hwnd| injection::InjectionTarget {
            hwnd,
            process_name: injection::process_name_for(hwnd),
        });
    injection::inject_text(&text, settings.restore_clipboard, preferred).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_dictionary_entry(
    state: State<'_, AppState>,
    mut entry: DictionaryEntry,
) -> CommandResult<AppSnapshot> {
    if entry.id.trim().is_empty() {
        entry.id = Uuid::new_v4().to_string();
    }
    let database = state.database.lock();
    database
        .upsert_dictionary_entry(&entry)
        .map_err(|e| e.to_string())?;
    database.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_dictionary_entry(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    database
        .delete_dictionary_entry(&id)
        .map_err(|e| e.to_string())?;
    database.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_snippet(
    state: State<'_, AppState>,
    mut snippet: Snippet,
) -> CommandResult<AppSnapshot> {
    if snippet.id.trim().is_empty() {
        snippet.id = Uuid::new_v4().to_string();
    }
    let database = state.database.lock();
    database
        .upsert_snippet(&snippet)
        .map_err(|e| e.to_string())?;
    database.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: String) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    database.delete_snippet(&id).map_err(|e| e.to_string())?;
    database.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_session(state: State<'_, AppState>, id: String) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    let audio_path = database.delete_session(&id).map_err(|e| e.to_string())?;
    let snapshot = database.snapshot().map_err(|e| e.to_string())?;
    drop(database);
    remove_managed_recordings(&state.app_dir, audio_path.into_iter().collect());
    Ok(snapshot)
}

fn remove_managed_recordings(app_dir: &std::path::Path, paths: Vec<String>) {
    let recordings_dir = app_dir.join("recordings");
    for raw_path in paths {
        let path = std::path::PathBuf::from(raw_path);
        if path.starts_with(&recordings_dir) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[tauri::command]
pub fn get_model_status(app: AppHandle, state: State<'_, AppState>) -> CommandResult<ModelStatus> {
    let settings = state
        .database
        .lock()
        .load_settings()
        .map_err(|e| e.to_string())?;
    Ok(runtime::model_status(&app, &settings))
}

#[tauri::command]
pub fn get_model_inventory(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<ModelInventory> {
    let settings = state
        .database
        .lock()
        .load_settings()
        .map_err(|e| e.to_string())?;
    Ok(runtime::model_inventory(&app, &settings))
}

#[tauri::command]
pub async fn download_model(app: AppHandle, model_id: String) -> CommandResult<ModelInventory> {
    let download_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        model_downloader::download(&download_app, &model_id)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;

    let state = app.state::<AppState>();
    state.shutdown_asr_host();
    crate::start_asr_host(&app);
    let settings = state
        .database
        .lock()
        .load_settings()
        .map_err(|error| error.to_string())?;
    Ok(runtime::model_inventory(&app, &settings))
}

#[tauri::command]
pub fn cancel_model_download(state: State<'_, AppState>) -> bool {
    model_downloader::cancel(&state)
}

#[tauri::command]
pub fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> CommandResult<ModelInventory> {
    model_downloader::delete(&app, &model_id).map_err(|error| error.to_string())?;
    let settings = state
        .database
        .lock()
        .load_settings()
        .map_err(|error| error.to_string())?;
    Ok(runtime::model_inventory(&app, &settings))
}
