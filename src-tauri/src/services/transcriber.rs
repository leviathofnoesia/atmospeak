use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, anyhow};
use tauri::AppHandle;

use crate::{
    models::AppSettings,
    services::{app_state::AppState, asr_host::AsrHost, metrics, runtime},
};

pub use crate::services::asr_host::DICTATION_CANCELLED;

/// A transcript plus which adapter produced it, so stage metrics can label the
/// run while the sidecar remains the only inference engine.
pub struct Transcription {
    pub text: String,
    pub backend: &'static str,
}

pub const EMPTY_TRANSCRIPT: &str = "No speech was detected in that recording.";

pub fn transcribe_cancellable(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
    cancel: &AtomicBool,
) -> Result<Transcription> {
    transcribe_inner(app, settings, wav_path, Some(cancel))
}

fn transcribe_inner(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<Transcription> {
    if is_cancelled(cancel) {
        return Err(anyhow!(DICTATION_CANCELLED));
    }

    let status = runtime::model_status(app, settings);
    if !status.ready {
        return Err(anyhow!(status.message));
    }

    if let Some(host) = AppState::asr_host_from(app) {
        let text = match cancel {
            Some(cancel) => host.transcribe_cancellable(wav_path, cancel),
            None => host.transcribe(wav_path),
        }?;
        return finish_transcript(text, metrics::ASR_BACKEND_HOST, cancel);
    }

    let text = match cancel {
        Some(cancel) => transcribe_with_cli_primitive_cancellable(app, settings, wav_path, cancel),
        None => transcribe_with_cli_primitive(app, settings, wav_path),
    }?;
    finish_transcript(text, metrics::ASR_BACKEND_CLI, cancel)
}

/// Transcribe a durable WAV through the resolved native sidecar for callers
/// that own a recording but do not keep an AppState host.
pub fn transcribe_with_cli_primitive(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
) -> Result<String> {
    transcribe_with_cli_primitive_inner(app, settings, wav_path, None)
}

pub fn transcribe_with_cli_primitive_cancellable(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
    cancel: &AtomicBool,
) -> Result<String> {
    transcribe_with_cli_primitive_inner(app, settings, wav_path, Some(cancel))
}

fn transcribe_with_cli_primitive_inner(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<String> {
    if is_cancelled(cancel) {
        return Err(anyhow!(DICTATION_CANCELLED));
    }

    let resolved = runtime::resolve_runtime(app, settings)?;
    let host = AsrHost::new(resolved.executable_path, resolved.model_path, resolved.speech_language)?;
    let text = match cancel {
        Some(cancel) => host.transcribe_cancellable(wav_path, cancel),
        None => host.transcribe(wav_path),
    }?;

    if is_cancelled(cancel) {
        return Err(anyhow!(DICTATION_CANCELLED));
    }
    if text.trim().is_empty() {
        return Err(anyhow!(EMPTY_TRANSCRIPT));
    }
    Ok(text)
}

fn finish_transcript(
    text: String,
    backend: &'static str,
    cancel: Option<&AtomicBool>,
) -> Result<Transcription> {
    if is_cancelled(cancel) {
        return Err(anyhow!(DICTATION_CANCELLED));
    }
    if text.trim().is_empty() {
        return Err(anyhow!(EMPTY_TRANSCRIPT));
    }
    Ok(Transcription { text, backend })
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|signal| signal.load(Ordering::Acquire))
}
