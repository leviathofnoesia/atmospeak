mod transfer;

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use atmospeak_asr_protocol::speech_models::{self, SpeechModel};
use reqwest::blocking::Client;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{ModelDownloadProgress, ModelInventoryItem},
    services::app_state::AppState,
};

pub(crate) use transfer::write_verified_stream;

pub fn installed_model_path(app_dir: &Path, model: &SpeechModel) -> PathBuf {
    app_dir.join("models").join(&model.filename)
}

pub fn is_model_file_ready(path: &Path) -> bool {
    speech_models::by_path(path).is_some_and(|model| {
        fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() == model.bytes)
    })
}

pub fn inventory_item(app_dir: &Path, model: &SpeechModel) -> ModelInventoryItem {
    let path = installed_model_path(app_dir, model);
    let installed = is_model_file_ready(&path);
    ModelInventoryItem {
        id: model.id.to_string(),
        label: model.label.to_string(),
        installed,
        bundled: false,
        path: installed.then(|| path.to_string_lossy().into_owned()),
        size_mb: Some(model.bytes / (1024 * 1024)),
        speech: Some(model.details.clone()),
    }
}

pub fn download(app: &AppHandle, model_id: &str) -> Result<()> {
    let model = speech_models::by_id(model_id).context("Unknown speech model")?;
    let state = app.state::<AppState>();
    let _operation = state
        .speech_model_operation
        .try_lock()
        .context("Another speech model operation is in progress")?;
    let destination = installed_model_path(&state.app_dir, model);
    if is_model_file_ready(&destination) {
        return Ok(());
    }

    state.begin_model_download(model_id)?;
    emit(
        app,
        model,
        "starting",
        0,
        format!("Starting {} download.", model.label),
    );
    let result = download_inner(app, &state, model, &destination);
    let cancelled = state.model_download_cancel.load(Ordering::Relaxed);
    state.finish_model_download();

    match &result {
        Ok(()) => emit(
            app,
            model,
            "installed",
            model.bytes,
            format!("{} is installed.", model.label),
        ),
        Err(_) if cancelled => emit(app, model, "cancelled", 0, "Download cancelled.".into()),
        Err(error) => emit(app, model, "error", 0, format!("{error:#}")),
    }
    result
}

fn download_inner(
    app: &AppHandle,
    state: &AppState,
    model: &SpeechModel,
    destination: &Path,
) -> Result<()> {
    let response = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent("Atmospeak speech model downloader")
        .build()?
        .get(&model.url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("Could not download {}", model.label))?;
    if let Some(bytes) = response.content_length() {
        ensure!(
            bytes == model.bytes,
            "Unexpected download size for {}",
            model.label
        );
    }
    let mut last_emit = None::<Instant>;
    write_verified_stream(
        response.take(model.bytes + 1),
        destination,
        &model.sha256,
        &state.model_download_cancel,
        |bytes| {
            if bytes == model.bytes {
                emit(
                    app,
                    model,
                    "verifying",
                    bytes,
                    format!("Verifying {}.", model.label),
                );
            } else if last_emit.is_none_or(|last| last.elapsed() >= Duration::from_millis(200)) {
                last_emit = Some(Instant::now());
                emit(
                    app,
                    model,
                    "downloading",
                    bytes,
                    format!("Downloading {}.", model.label),
                );
            }
        },
    )
}

pub fn cancel(state: &AppState) -> bool {
    state.cancel_model_download()
}

pub fn delete(state: &AppState, model_id: &str, active_model_id: &str) -> Result<()> {
    let model = speech_models::by_id(model_id).context("Unknown speech model")?;
    if model_id == active_model_id {
        bail!(
            "Switch to another installed model before deleting {}.",
            model.label
        );
    }
    let path = installed_model_path(&state.app_dir, model);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Could not delete {}", model.label)),
    }
}

fn emit(app: &AppHandle, model: &SpeechModel, status: &str, bytes: u64, message: String) {
    let _ = app.emit(
        "atmospeak://model-download",
        ModelDownloadProgress {
            model_id: model.id.to_string(),
            status: status.to_string(),
            bytes_downloaded: bytes,
            total_bytes: Some(model.bytes),
            percent: Some((bytes as f64 / model.bytes as f64 * 100.0).min(100.0)),
            message,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use atmospeak_asr_protocol::PARAKEET_MODEL_ID;

    #[test]
    fn managed_models_live_under_the_app_models_directory() {
        let root = Path::new("C:/test/Atmospeak");
        let model = speech_models::by_id(PARAKEET_MODEL_ID).expect("Parakeet model");
        assert_eq!(
            installed_model_path(root, model),
            root.join("models").join(&model.filename)
        );
    }

    #[test]
    fn managed_model_ids_and_filenames_are_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut filenames = std::collections::HashSet::new();
        for model in speech_models::models() {
            assert!(ids.insert(&model.id), "duplicate model id: {}", model.id);
            assert!(
                filenames.insert(&model.filename),
                "duplicate model file: {}",
                model.filename
            );
        }
    }
}
