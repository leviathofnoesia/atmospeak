//! Curated GGUF models for the bundled local polish runtime.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{ModelDownloadProgress, ModelInventoryItem},
    services::{app_state::AppState, llama_host, model_downloader},
};

pub const DEFAULT_POLISH_MODEL_ID: &str = "qwen2.5-0.5b";

#[derive(Debug, Clone, Copy)]
pub struct PolishModelDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub recommended: bool,
}

pub const POLISH_MODELS: &[PolishModelDescriptor] = &[
    PolishModelDescriptor {
        id: DEFAULT_POLISH_MODEL_ID,
        label: "Fast (Qwen2.5 0.5B)",
        filename: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf",
        sha256: "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db",
        size_bytes: 491_000_000,
        recommended: true,
    },
];

pub fn descriptor(model_id: &str) -> Option<&'static PolishModelDescriptor> {
    POLISH_MODELS.iter().find(|model| model.id == model_id)
}

pub fn models_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("polish-models")
}

pub fn installed_path(app_dir: &Path, model: &PolishModelDescriptor) -> PathBuf {
    models_dir(app_dir).join(model.filename)
}

pub fn inventory(app_dir: &Path) -> Vec<ModelInventoryItem> {
    let mut models = POLISH_MODELS.to_vec();
    models.sort_by_key(|model| !model.recommended);
    models
        .iter()
        .map(|model| {
            let path = installed_path(app_dir, model);
            let installed = path.is_file();
            let label = if model.recommended {
                format!("{} · recommended", model.label)
            } else {
                model.label.to_string()
            };
            ModelInventoryItem {
                id: model.id.to_string(),
                label,
                installed,
                bundled: false,
                path: installed.then(|| path.to_string_lossy().to_string()),
                size_mb: Some((model.size_bytes + 1024 * 1024 - 1) / (1024 * 1024)),
            }
        })
        .collect()
}

pub fn resolve_model_path(app_dir: &Path, model_id: &str) -> Result<PathBuf> {
    let model = descriptor(model_id).ok_or_else(|| anyhow!("unknown polish model: {model_id}"))?;
    let path = installed_path(app_dir, model);
    if !path.is_file() {
        bail!(
            "{} is not installed yet. Download it from Settings → AI edit.",
            model.label
        );
    }
    Ok(path)
}

pub fn download(app: &AppHandle, model_id: &str) -> Result<()> {
    let model = descriptor(model_id).ok_or_else(|| anyhow!("unknown polish model: {model_id}"))?;
    let state = app.state::<AppState>();
    state.begin_model_download(model_id)?;
    let result = download_inner(app, &state, model);
    state.finish_model_download();

    match &result {
        Ok(()) => emit(
            app,
            progress(
                model,
                "installed",
                model.size_bytes,
                Some(model.size_bytes),
                format!("{} is installed and ready.", model.label),
            ),
        ),
        Err(error) if error.to_string().contains("cancelled") => emit(
            app,
            progress(
                model,
                "cancelled",
                0,
                Some(model.size_bytes),
                format!("{} download cancelled.", model.label),
            ),
        ),
        Err(error) => emit(
            app,
            progress(model, "error", 0, Some(model.size_bytes), error.to_string()),
        ),
    }
    result
}

fn download_inner(
    app: &AppHandle,
    state: &AppState,
    model: &PolishModelDescriptor,
) -> Result<()> {
    let destination = installed_path(&state.app_dir, model);
    fs::create_dir_all(models_dir(&state.app_dir))
        .context("failed to create polish-models directory")?;

    emit(
        app,
        progress(
            model,
            "starting",
            0,
            Some(model.size_bytes),
            format!("Starting {} download.", model.label),
        ),
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(60 * 60))
        .user_agent("Atmospeak polish model downloader")
        .build()
        .context("failed to create polish model download client")?;
    let response = client
        .get(model.url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("failed to download {}", model.label))?;
    let total = response.content_length().or(Some(model.size_bytes));
    let mut last_emit = Instant::now() - Duration::from_secs(1);

    model_downloader::write_verified_stream(
        response,
        &destination,
        model.sha256,
        &state.model_download_cancel,
        |bytes| {
            if last_emit.elapsed() >= Duration::from_millis(200) || Some(bytes) == total {
                last_emit = Instant::now();
                emit(
                    app,
                    progress(
                        model,
                        "downloading",
                        bytes,
                        total,
                        format!("Downloading {}.", model.label),
                    ),
                );
            }
        },
    )?;

    emit(
        app,
        progress(
            model,
            "verifying",
            model.size_bytes,
            total,
            format!("Verified {}.", model.label),
        ),
    );
    Ok(())
}

/// True when model + server binary already exist locally (no network).
pub fn runtime_ready(app: &AppHandle, model_id: &str) -> bool {
    let state = app.state::<AppState>();
    let Ok(model_path) = resolve_model_path(&state.app_dir, model_id) else {
        return false;
    };
    if !model_path.is_file() {
        return false;
    }
    llama_host::resolve_server_exe(app).is_some_and(|path| path.is_file())
}

/// Publish a lazy host when binaries/models are already on disk. Never downloads.
pub fn attach_ready_runtime(app: &AppHandle, model_id: &str) -> Result<()> {
    let state = app.state::<AppState>();
    let model_path = resolve_model_path(&state.app_dir, model_id)?;
    if let Some(host) = state.llama_host() {
        if host.model_path() == model_path.as_path() {
            return Ok(());
        }
    }
    let server = llama_host::resolve_server_exe(app)
        .ok_or_else(|| anyhow!("llama-server is not installed yet"))?;
    if !server.is_file() {
        bail!("llama-server is not installed yet");
    }
    state.shutdown_llama_host();
    llama_host::publish_host(app, model_path)?;
    Ok(())
}

/// Ensure server binary + selected model exist, then publish a lazy LlamaHost.
/// May download — use only from Settings setup, never from the paste hot path.
pub fn ensure_runtime(app: &AppHandle, model_id: &str) -> Result<()> {
    let state = app.state::<AppState>();
    let model_path = match resolve_model_path(&state.app_dir, model_id) {
        Ok(path) => path,
        Err(_) => {
            download(app, model_id)?;
            resolve_model_path(&state.app_dir, model_id)?
        }
    };
    llama_host::ensure_server_binary(app)?;
    if let Some(host) = state.llama_host() {
        if host.model_path() == model_path.as_path() {
            return Ok(());
        }
    }
    state.shutdown_llama_host();
    llama_host::publish_host(app, model_path)?;
    Ok(())
}

fn progress(
    model: &PolishModelDescriptor,
    status: &str,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
    message: String,
) -> ModelDownloadProgress {
    ModelDownloadProgress {
        model_id: model.id.to_string(),
        status: status.to_string(),
        bytes_downloaded,
        total_bytes,
        percent: total_bytes
            .filter(|total| *total > 0)
            .map(|total| (bytes_downloaded as f64 / total as f64 * 100.0).min(100.0)),
        message,
    }
}

fn emit(app: &AppHandle, payload: ModelDownloadProgress) {
    let _ = app.emit("atmospeak://polish-model-download", payload.clone());
    let _ = app.emit("atmospeak://model-download", payload);
}
