use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    models::{
        AppSettings, AppSnapshot, DictationResult, DictionaryEntry, InjectionResult,
        MicrophoneInfo, ModelInventory, ModelStatus, RecordingStarted, RuntimeEvent,
        ShortcutStatus, Snippet, StageMetrics,
    },
    services::{
        app_state::AppState,
        dictation_engine::{self, DispatchResult, EngineAction},
        injection, overlay_window, runtime, shortcuts, startup,
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
    state.recorder.list_microphones().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> CommandResult<AppSnapshot> {
    startup::set_start_at_login(settings.start_at_login).map_err(|e| e.to_string())?;
    let database = state.database.lock();
    database
        .save_settings(&settings)
        .map_err(|e| e.to_string())?;
    let _ = runtime::model_status(&app, &settings);
    shortcuts::register_shortcut(
        &app,
        state.shortcut_status.clone(),
        state.shortcuts_paused.clone(),
        &settings.hotkey,
        *state.shortcuts_paused.lock(),
    );
    // The overlay lives in its own webview and reads settings once on mount, so
    // appearance and hotkey changes have to be pushed to it.
    let _ = app.emit("wind-speak://settings-changed", settings.clone());
    let _ = app.emit("atmospeak://settings-changed", settings);
    database.snapshot().map_err(|e| e.to_string())
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
pub fn show_overlay_window(app: AppHandle) -> CommandResult<()> {
    overlay_window::show_and_reset(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub fn save_overlay_position(app: AppHandle, x: i32, y: i32) {
    overlay_window::save_position(&app, x, y);
}

#[tauri::command]
pub fn set_shortcut_test_active(state: State<'_, AppState>, active: bool) {
    state.set_shortcut_test_active(active);
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
    match engine.dispatch_with_ack(engine_action)? {
        DispatchResult::Accepted => Ok("accepted".to_string()),
        DispatchResult::Ignored { reason } => Ok(format!("ignored:{reason}")),
        DispatchResult::Rejected { reason } => Err(reason),
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
    database.upsert_snippet(&snippet).map_err(|e| e.to_string())?;
    database.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: String) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    database.delete_snippet(&id).map_err(|e| e.to_string())?;
    database.snapshot().map_err(|e| e.to_string())
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
