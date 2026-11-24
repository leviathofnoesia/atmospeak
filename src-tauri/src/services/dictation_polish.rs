use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc,
    },
    thread,
};

use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{AppSettings, TranscriptSession},
    services::{app_state::AppState, metrics, polish},
};

/// Optional copy-only work owns neither the recorder nor the delivery cancellation flag.
pub struct HistoryPolishWorker {
    sender: SyncSender<(AppSettings, TranscriptSession)>,
    shutdown: Arc<AtomicBool>,
}

impl HistoryPolishWorker {
    pub fn spawn(app: AppHandle) -> std::io::Result<Self> {
        Self::start(move |settings, session, cancelled| {
            let Some(text) = run_auto_polish(
                &app,
                &settings,
                &session.cleaned_text,
                &session.id,
                cancelled,
            ) else {
                return;
            };
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            let saved = app
                .state::<AppState>()
                .database
                .lock()
                .save_automatic_polish(&session, &text);
            match saved {
                Ok(true) => {
                    let _ = app.emit("atmospeak://history-changed", ());
                }
                Ok(false) => {}
                Err(error) => metrics::emit_runtime(&app, "polish-save-error", error.to_string()),
            }
        })
    }

    pub(super) fn start(
        process: impl Fn(AppSettings, TranscriptSession, &AtomicBool) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<(AppSettings, TranscriptSession)>(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        let cancelled = shutdown.clone();
        thread::Builder::new()
            .name("atmospeak-history-polish".into())
            .spawn(move || {
                while let Ok((settings, session)) = receiver.recv() {
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    process(settings, session, &cancelled);
                }
            })?;
        Ok(Self { sender, shutdown })
    }

    /// At most one request runs and one waits. Optional work never blocks delivery.
    pub fn submit(&self, settings: AppSettings, session: TranscriptSession) -> bool {
        self.sender.try_send((settings, session)).is_ok()
    }
}

impl Drop for HistoryPolishWorker {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

pub fn run_auto_polish(
    app: &AppHandle,
    settings: &AppSettings,
    text: &str,
    session_id: &str,
    cancel: &AtomicBool,
) -> Option<String> {
    match polish::polish_if_enabled_cancellable(
        app,
        settings,
        text,
        polish::AUTO_POLISH_TIMEOUT,
        cancel,
    ) {
        Ok(Some(outcome)) => {
            metrics::emit_runtime(
                app,
                "polish-ok",
                format!("session={session_id} elapsed_ms={}", outcome.elapsed_ms),
            );
            Some(outcome.text)
        }
        Ok(None) => None,
        Err(error) => {
            if cancel.load(Ordering::Acquire) || error.to_string() == "dictation cancelled" {
                return None;
            }
            let kind = if polish::is_timeout_error(&error) {
                "polish-timeout"
            } else {
                "polish-fallback"
            };
            metrics::emit_runtime(
                app,
                kind,
                format!(
                    "session={session_id} {}",
                    polish::sanitize_message(&error.to_string())
                ),
            );
            None
        }
    }
}
