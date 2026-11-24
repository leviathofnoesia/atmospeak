use crate::{
    models::AppSettings,
    services::{app_state::AppState, dictation_engine::EngineAction, polish},
};
use anyhow::{Result, bail};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

/// Plain case commands need neither a model request nor a provider startup.
pub fn direct_edit(text: &str, instruction: &str) -> Option<String> {
    let command = instruction
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_lowercase();
    match command.as_str() {
        "uppercase" | "upper case" | "all caps" | "make this uppercase" | "make it uppercase" => {
            Some(text.to_uppercase())
        }
        "lowercase" | "lower case" | "make this lowercase" | "make it lowercase" => {
            Some(text.to_lowercase())
        }
        _ => None,
    }
}

pub fn rewrite(
    app: &AppHandle,
    settings: &AppSettings,
    text: &str,
    instruction: &str,
    cancel: &AtomicBool,
) -> Result<String> {
    if cancel.load(Ordering::Acquire) {
        bail!("dictation cancelled");
    }
    if let Some(edited) = direct_edit(text, instruction) {
        return Ok(edited);
    }
    polish::edit_selection_cancellable(app, settings, text, instruction, cancel)
}

pub fn validate_hotkey(settings: &AppSettings) -> Result<()> {
    if settings.voice_edit_hotkey.trim().is_empty() {
        return Ok(());
    }
    let edit: tauri_plugin_global_shortcut::Shortcut = settings.voice_edit_hotkey.parse()?;
    if settings
        .hotkey
        .parse::<tauri_plugin_global_shortcut::Shortcut>()
        .ok()
        .as_ref()
        == Some(&edit)
    {
        bail!("Dictation and voice editing need different shortcuts.");
    }
    Ok(())
}

#[cfg(windows)]
pub fn install(app: &AppHandle) -> Result<()> {
    app.plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
    Ok(())
}
#[cfg(not(windows))]
pub fn install(_app: &AppHandle) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
pub fn register(app: &AppHandle, previous: &str, requested: &str) -> Result<()> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let manager = app.global_shortcut();
    if !previous.trim().is_empty() {
        if let Ok(previous) = previous.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            let _ = manager.unregister(previous);
        }
    }
    if requested.trim().is_empty() {
        return Ok(());
    }
    let shortcut: tauri_plugin_global_shortcut::Shortcut = requested.parse()?;
    manager.on_shortcut(shortcut, |app, _, event| {
        let state = app.state::<AppState>();
        if event.state() == ShortcutState::Pressed
            && !*state.shortcuts_paused.lock()
            && !state.shortcut_test_active()
            && !state.shortcut_capture_active()
        {
            state
                .engine()
                .map(|engine| engine.dispatch_fire_and_forget(EngineAction::EditSelection));
        }
    })?;
    Ok(())
}
#[cfg(not(windows))]
pub fn register(_app: &AppHandle, _previous: &str, _requested: &str) -> Result<()> {
    Ok(())
}
