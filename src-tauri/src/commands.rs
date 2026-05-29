use anyhow::{Context, Result};
use chrono::Utc;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    models::{
        AppSettings, AppSnapshot, DictationResult, DictionaryEntry, InjectionResult, MicrophoneInfo,
        ModelInventory, ModelStatus, RecordingStarted, ShortcutStatus, Snippet, TranscriptSession,
    },
    services::{
        app_state::AppState, cleanup, injection, recorder::FinishedRecording, runtime, shortcuts,
        startup, transcriber,
    },
};

type CommandResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshot> {
    to_command_result(state.database.lock().snapshot())
}

#[tauri::command]
pub fn get_shortcut_status(state: State<'_, AppState>) -> CommandResult<ShortcutStatus> {
    Ok(state.shortcut_status.lock().clone())
}

#[tauri::command]
pub fn list_microphones(state: State<'_, AppState>) -> CommandResult<Vec<MicrophoneInfo>> {
    to_command_result(state.recorder.list_microphones())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    to_command_result(
        startup::set_start_at_login(settings.start_at_login)
            .and_then(|_| database.save_settings(&settings))
            .and_then(|_| {
                let _ = runtime::model_status(&app, &settings);
                shortcuts::register_shortcut(&app, state.shortcut_status.clone(), &settings.hotkey);
                database.snapshot()
            }),
    )
}

#[tauri::command]
pub fn start_recording(state: State<'_, AppState>) -> CommandResult<RecordingStarted> {
    let settings = to_command_result(state.database.lock().load_settings())?;
    to_command_result(state.recorder.start(settings.microphone_name))
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<DictationResult> {
    let finished = to_command_result(state.recorder.stop())?;
    let snapshot = to_command_result(state.database.lock().snapshot())?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        complete_recording_inner(&app, &snapshot, finished)
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(to_command_result)?;

    to_command_result(
        state
            .database
            .lock()
            .insert_session(&result.session)
            .context("failed to save transcript session"),
    )?;

    Ok(result)
}

#[tauri::command]
pub fn cancel_recording(state: State<'_, AppState>) -> CommandResult<()> {
    to_command_result(state.recorder.cancel())
}

#[tauri::command]
pub fn inject_text(state: State<'_, AppState>, text: String) -> CommandResult<InjectionResult> {
    let settings = to_command_result(state.database.lock().load_settings())?;
    to_command_result(injection::inject_text(&text, settings.restore_clipboard))
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
    to_command_result(
        database
            .upsert_dictionary_entry(&entry)
            .and_then(|_| database.snapshot()),
    )
}

#[tauri::command]
pub fn delete_dictionary_entry(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    to_command_result(
        database
            .delete_dictionary_entry(&id)
            .and_then(|_| database.snapshot()),
    )
}

#[tauri::command]
pub fn upsert_snippet(state: State<'_, AppState>, mut snippet: Snippet) -> CommandResult<AppSnapshot> {
    if snippet.id.trim().is_empty() {
        snippet.id = Uuid::new_v4().to_string();
    }

    let database = state.database.lock();
    to_command_result(
        database
            .upsert_snippet(&snippet)
            .and_then(|_| database.snapshot()),
    )
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: String) -> CommandResult<AppSnapshot> {
    let database = state.database.lock();
    to_command_result(database.delete_snippet(&id).and_then(|_| database.snapshot()))
}

#[tauri::command]
pub fn get_model_status(app: AppHandle, state: State<'_, AppState>) -> CommandResult<ModelStatus> {
    let settings = to_command_result(state.database.lock().load_settings())?;
    Ok(runtime::model_status(&app, &settings))
}

#[tauri::command]
pub fn get_model_inventory(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<ModelInventory> {
    let settings = to_command_result(state.database.lock().load_settings())?;
    Ok(runtime::model_inventory(&app, &settings))
}

fn complete_recording_inner(
    app: &AppHandle,
    snapshot: &AppSnapshot,
    finished: FinishedRecording,
) -> Result<DictationResult> {
    let raw_text = transcriber::transcribe(app, &snapshot.settings, &finished.path)?;
    let cleaned_text = if snapshot.settings.cleanup_enabled {
        cleanup::clean_text(&raw_text, &snapshot.dictionary, &snapshot.snippets)
    } else {
        raw_text.trim().to_string()
    };
    let injection_result = if snapshot.settings.auto_inject {
        Some(injection::inject_text(
            &cleaned_text,
            snapshot.settings.restore_clipboard,
        )?)
    } else {
        None
    };
    let session = TranscriptSession {
        id: finished.id,
        raw_text,
        word_count: cleaned_text.split_whitespace().count(),
        cleaned_text,
        audio_path: finished.path.to_string_lossy().to_string(),
        duration_ms: finished.duration_ms,
        injected: injection_result
            .as_ref()
            .map(|result| result.injected)
            .unwrap_or(false),
        created_at: Utc::now(),
    };

    Ok(DictationResult {
        session,
        injection: injection_result,
    })
}

fn to_command_result<T>(result: Result<T>) -> CommandResult<T> {
    result.map_err(|error| error.to_string())
}
