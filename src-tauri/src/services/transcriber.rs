use std::{path::Path, process::Command};

use anyhow::{Context, Result, anyhow};
use tauri::AppHandle;

use crate::{models::AppSettings, services::runtime};

pub fn transcribe(app: &AppHandle, settings: &AppSettings, wav_path: &Path) -> Result<String> {
    let status = runtime::model_status(app, settings);
    if !status.ready {
        return Err(anyhow!(status.message));
    }

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
        Err(anyhow!("whisper.cpp returned an empty transcript"))
    } else {
        Ok(transcript)
    }
}
