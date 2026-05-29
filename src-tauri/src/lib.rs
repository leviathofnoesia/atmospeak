mod commands;
mod db;
mod models;
mod services;
mod tray;

use anyhow::anyhow;
use tauri::Emitter;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use commands::{
    cancel_recording, delete_dictionary_entry, delete_snippet, get_app_snapshot, get_model_inventory,
    get_model_status, inject_text, list_microphones, save_settings, start_recording, stop_recording,
    upsert_dictionary_entry, upsert_snippet,
};
use services::app_state::AppState;

fn install_global_shortcut(app: &mut tauri::App) -> anyhow::Result<()> {
    #[cfg(desktop)]
    {
        let preferred_shortcut =
            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::Space);
        let fallback_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
        let shortcuts_for_handler = vec![preferred_shortcut.clone(), fallback_shortcut.clone()];

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
        if let Err(error) = app.global_shortcut().register(preferred_shortcut) {
            eprintln!(
                "failed to register Ctrl+Win+Space, falling back to Ctrl+Alt+Space: {error}"
            );
            app.global_shortcut()
                .register(fallback_shortcut)
                .map_err(|error| anyhow!(error.to_string()))?;
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            install_global_shortcut(app)?;
            tray::install(app)?;
            Ok(())
        })
        .manage(AppState::new().expect("failed to initialize application state"))
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
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
