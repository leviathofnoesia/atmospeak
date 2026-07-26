use std::{path::Path, process::Command};

use anyhow::{Context, Result, anyhow};
use tauri::AppHandle;

use crate::{
    models::AppSettings,
    services::{app_state::AppState, metrics, proc, runtime},
};

/// A transcript plus which backend produced it, so stage metrics can label the run.
pub struct Transcription {
    pub text: String,
    pub backend: &'static str,
}

/// Transcribe a recording, preferring the resident host and degrading to the
/// one-shot CLI whenever the host is unavailable, disabled, or failing.
pub fn transcribe(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
) -> Result<Transcription> {
    let status = runtime::model_status(app, settings);
    if !status.ready {
        return Err(anyhow!(status.message));
    }

    if let Some(host) = AppState::asr_host_from(app) {
        match host.transcribe(wav_path) {
            // An empty transcript means no speech, not a broken host. Retrying on the
            // CLI would only spend seconds arriving at the same answer.
            Ok(text) if text.is_empty() => return Err(anyhow!(EMPTY_TRANSCRIPT)),
            Ok(text) => {
                return Ok(Transcription {
                    text,
                    backend: metrics::ASR_BACKEND_HOST,
                });
            }
            Err(error) => {
                // Never fail the utterance because the sidecar misbehaved.
                metrics::emit_runtime(
                    app,
                    "asr-host-fallback",
                    format!("resident host failed, using CLI for this utterance: {error}"),
                );
            }
        }
    }

    Ok(Transcription {
        text: transcribe_with_cli(app, settings, wav_path)?,
        backend: metrics::ASR_BACKEND_CLI,
    })
}

const EMPTY_TRANSCRIPT: &str = "No speech was detected in that recording.";

fn transcribe_with_cli(
    app: &AppHandle,
    settings: &AppSettings,
    wav_path: &Path,
) -> Result<String> {
    let resolved = runtime::resolve_runtime(app, settings)?;
    let wav = wav_path
        .to_str()
        .ok_or_else(|| anyhow!("recording path contains invalid unicode"))?;
    let model = resolved
        .model_path
        .to_str()
        .ok_or_else(|| anyhow!("model path contains invalid unicode"))?;
    let mut command = Command::new(&resolved.whisper_cli_path);
    if let Some(runtime_dir) = resolved.whisper_cli_path.parent() {
        command.current_dir(runtime_dir);
    }
    proc::hide_console(&mut command);

    let output = command
        .args(["-m", model, "-f", wav, "-nt", "-np"])
        .output()
        .context("failed to run whisper.cpp CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("whisper.cpp failed: {}", stderr.trim()));
    }

    let transcript = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if transcript.is_empty() {
        Err(anyhow!(EMPTY_TRANSCRIPT))
    } else {
        Ok(transcript)
    }
}
