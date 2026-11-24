use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    models::TranscriptSession,
    services::{app_state::AppState, window_manager},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRequest {
    pub generation: u64,
    pub session_id: Option<String>,
}

pub(crate) fn copyable_text(session: &TranscriptSession) -> Option<&str> {
    let preferred = if session.prefer_polished {
        session.polished_text.as_deref()
    } else {
        None
    };
    preferred
        .into_iter()
        .chain([session.cleaned_text.as_str(), session.raw_text.as_str()])
        .find(|text| !text.trim().is_empty())
}

pub fn copy_transcript(app: &AppHandle, id: Option<&str>) -> Result<String, String> {
    let session = {
        let state = app.state::<AppState>();
        let db = state.database.lock();
        match id {
            Some(id) => db.get_session(id),
            None => db.latest_session(),
        }
        .map_err(|error| error.to_string())?
        .ok_or("No saved transcript is available. Open History to check your recordings.")?
    };
    let text = copyable_text(&session)
        .ok_or("This recording has no recovered text yet. Open it in History to listen.")?;
    app.clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())?;
    Ok("Transcript copied. Paste it where you need it.".into())
}

pub fn open_history(app: &AppHandle, session_id: Option<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(id) = session_id.as_deref() {
        if state
            .database
            .lock()
            .get_session(id)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("This recording is no longer in History.".into());
        }
    }
    {
        let mut pending = state.history_request.lock();
        let generation = state.history_request_generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        *pending = Some(HistoryRequest { generation, session_id });
    }
    let setup = !window_manager::setup_is_complete(app, crate::ONBOARDING_VERSION);
    let window = window_manager::ensure_main(app, setup).map_err(|error| error.to_string())?;
    // The pending request is also consumed when a newly created hub finishes mounting.
    window
        .emit("atmospeak://open-history", ())
        .map_err(|error| error.to_string())
}
