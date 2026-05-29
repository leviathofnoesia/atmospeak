mod commands;
mod db;
mod models;
mod services;
mod tray;

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::Emitter;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use commands::{
    cancel_recording, delete_dictionary_entry, delete_snippet, get_app_snapshot, get_model_inventory,
    get_model_status, get_shortcut_status, inject_text, list_microphones, save_settings,
    start_recording, stop_recording, upsert_dictionary_entry, upsert_snippet,
};
use models::ShortcutStatus;
use services::app_state::AppState;

fn install_global_shortcut(
    app: &mut tauri::App,
    shortcut_status: Arc<Mutex<ShortcutStatus>>,
) -> anyhow::Result<()> {
    #[cfg(desktop)]
    {
        let shortcut_candidates = vec![
            (
                "Ctrl+Win+Space",
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::Space),
            ),
            (
                "Ctrl+Alt+Space",
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space),
            ),
            (
                "Ctrl+Shift+Space",
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space),
            ),
        ];
        let shortcuts_for_handler = shortcut_candidates
            .iter()
            .map(|(_, shortcut)| shortcut.clone())
            .collect::<Vec<_>>();

        app.handle().plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if shortcuts_for_handler.iter().any(|candidate| shortcut == candidate) {
                        let payload = match event.state() {
                            ShortcutState::Pressed => "pressed",
                            ShortcutState::Released => "released",
                        };
                        let _ = app.emit("wind-speak://shortcut", payload);
                    }
                })
                .build(),
        )?;

        let mut failures = Vec::new();
        for (label, shortcut) in shortcut_candidates {
            match app.global_shortcut().register(shortcut) {
                Ok(()) => {
                    let status = ShortcutStatus {
                        registered: true,
                        hotkey: label.to_string(),
                        message: format!("Global shortcut registered: {label}"),
                    };
                    *shortcut_status.lock() = status.clone();
                    let _ = app.emit("wind-speak://shortcut-status", status);
                    return Ok(());
                }
                Err(error) => {
                    failures.push(format!("{label}: {error}"));
                }
            }
        }

        let status = ShortcutStatus {
            registered: false,
            hotkey: String::new(),
            message: format!(
                "Global shortcut unavailable. Use the overlay button or change conflicting system shortcuts. {}",
                failures.join(" / ")
            ),
        };
        eprintln!("{}", status.message);
        *shortcut_status.lock() = status.clone();
        let _ = app.emit("wind-speak://shortcut-status", status);
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new().expect("failed to initialize application state");
    let shortcut_status = app_state.shortcut_status.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            install_global_shortcut(app, shortcut_status.clone())?;
            tray::install(app)?;
            Ok(())
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_shortcut_status,
            list_microphones,
            save_settings,
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
