use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::{
    models::{ModelDownloadProgress, ModelInventoryItem},
    services::app_state::AppState,
};

pub const BUNDLED_MODEL_ID: &str = "base.en";

#[derive(Debug, Clone, Copy)]
pub struct ModelDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub bundled: bool,
}

pub const MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: BUNDLED_MODEL_ID,
        label: "Base English",
        filename: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        size_bytes: 147_964_211,
        bundled: true,
    },
    ModelDescriptor {
        id: "tiny.en",
        label: "Tiny English",
        filename: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        size_bytes: 77_704_715,
        bundled: false,
    },
    ModelDescriptor {
        id: "small.en",
        label: "Small English",
        filename: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        size_bytes: 487_614_201,
        bundled: false,
    },
    ModelDescriptor {
        id: "medium.en",
        label: "Medium English",
        filename: "ggml-medium.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
        sha256: "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356",
        size_bytes: 1_533_774_781,
        bundled: false,
    },
    ModelDescriptor {
        id: "distil-large-v3",
        label: "Distil Large v3",
        filename: "ggml-distil-large-v3.bin",
        url: "https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin",
        sha256: "2883a11b90fb10ed592d826edeaee7d2929bf1ab985109fe9e1e7b4d2b69a298",
        size_bytes: 1_519_521_155,
        bundled: false,
    },
    ModelDescriptor {
        id: "large-v3-turbo-q5",
        label: "Large v3 Turbo q5",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_bytes: 574_041_195,
        bundled: false,
    },
    ModelDescriptor {
        id: "distil-large-v3.5",
        label: "Distil Large v3.5",
        filename: "ggml-distil-large-v3.5.bin",
        url: "https://huggingface.co/distil-whisper/distil-large-v3.5-ggml/resolve/main/ggml-model.bin",
        sha256: "ec2498919b498c5f6b00041adb45650124b3cd9f26f545fffa8f5d11c28dcf26",
        size_bytes: 1_519_521_155,
        bundled: false,
    },
];

pub fn descriptor(model_id: &str) -> Option<&'static ModelDescriptor> {
    MODELS.iter().find(|model| model.id == model_id)
}

pub fn models_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("models")
}

pub fn installed_model_path(app_dir: &Path, model: &ModelDescriptor) -> PathBuf {
    models_dir(app_dir).join(model.filename)
}

pub fn inventory_item(
    app_dir: &Path,
    model: &ModelDescriptor,
    bundled_path: Option<&Path>,
) -> ModelInventoryItem {
    let path = if model.bundled {
        bundled_path.map(Path::to_path_buf)
    } else {
        Some(installed_model_path(app_dir, model))
    };
    let installed = path.as_ref().is_some_and(|candidate| candidate.is_file());

    ModelInventoryItem {
        id: model.id.to_string(),
        label: model.label.to_string(),
        installed,
        bundled: model.bundled,
        path: installed.then(|| path.unwrap().to_string_lossy().to_string()),
        size_mb: Some((model.size_bytes + 1024 * 1024 - 1) / (1024 * 1024)),
    }
}

pub fn download(app: &AppHandle, model_id: &str) -> Result<()> {
    let model = descriptor(model_id).ok_or_else(|| anyhow!("unknown model: {model_id}"))?;
    if model.bundled {
        bail!(
            "{} is bundled and does not need to be downloaded",
            model.label
        );
    }

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

fn download_inner(app: &AppHandle, state: &AppState, model: &ModelDescriptor) -> Result<()> {
    let destination = installed_model_path(&state.app_dir, model);
    fs::create_dir_all(models_dir(&state.app_dir))
        .context("failed to create the model directory")?;

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
        .user_agent("Atmospeak model downloader")
        .build()
        .context("failed to create the model download client")?;
    let response = client
        .get(model.url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("failed to download {}", model.label))?;
    let total = response.content_length().or(Some(model.size_bytes));
    let mut last_emit = Instant::now() - Duration::from_secs(1);

    write_verified_stream(
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

pub fn cancel(state: &AppState) -> bool {
    state.cancel_model_download()
}

pub fn delete(app: &AppHandle, model_id: &str) -> Result<()> {
    let model = descriptor(model_id).ok_or_else(|| anyhow!("unknown model: {model_id}"))?;
    if model.bundled {
        bail!("the bundled Base English model cannot be deleted");
    }

    let state = app.state::<AppState>();
    state.shutdown_asr_host();
    let path = installed_model_path(&state.app_dir, model);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to delete {}", path.display()))?;
    }
    crate::start_asr_host(app);
    Ok(())
}

fn progress(
    model: &ModelDescriptor,
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
    let _ = app.emit("wind-speak://model-download", payload.clone());
    let _ = app.emit("atmospeak://model-download", payload);
}

/// Write a response body to disk without checksum verification (zip installs, etc.).
pub(crate) fn write_stream_unchecked(mut response: impl Read, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(destination)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
    }
    file.flush()?;
    Ok(())
}

pub(crate) fn write_verified_stream<R, F>(
    mut reader: R,
    destination: &Path,
    expected_sha256: &str,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<()>
where
    R: Read,
    F: FnMut(u64),
{
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("model destination has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(
        ".{}.download-{}.tmp",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model"),
        Uuid::new_v4()
    ));
    let result = (|| {
        let mut temp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .context("failed to create temporary model file")?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut downloaded = 0_u64;

        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("model download cancelled");
            }
            let read = reader
                .read(&mut buffer)
                .context("failed while reading the model download")?;
            if read == 0 {
                break;
            }
            temp.write_all(&buffer[..read])
                .context("failed while writing the model download")?;
            hasher.update(&buffer[..read]);
            downloaded += read as u64;
            on_progress(downloaded);
        }

        temp.flush()?;
        temp.sync_all()?;
        drop(temp);

        let actual_sha256 = format!("{:x}", hasher.finalize());
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            bail!("checksum verification failed: expected {expected_sha256}, got {actual_sha256}");
        }
        atomic_replace(&temp_path, destination)
            .context("failed to install the verified model atomically")
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(target_os = "windows")]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(Into::into)
    }
}

#[cfg(not(target_os = "windows"))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn managed_models_live_under_the_app_models_directory() {
        let root = Path::new("C:/test/Atmospeak");
        let model = descriptor("small.en").expect("small model");
        assert_eq!(
            installed_model_path(root, model),
            root.join("models").join("ggml-small.en.bin")
        );
    }

    #[test]
    fn current_generation_models_use_pinned_hugging_face_blobs() {
        let turbo = descriptor("large-v3-turbo-q5").expect("large v3 turbo q5");
        assert_eq!(turbo.filename, "ggml-large-v3-turbo-q5_0.bin");
        assert_eq!(turbo.size_bytes, 574_041_195);
        assert_eq!(
            turbo.sha256,
            "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"
        );

        let distil = descriptor("distil-large-v3.5").expect("distil large v3.5");
        assert_eq!(distil.filename, "ggml-distil-large-v3.5.bin");
        assert_eq!(distil.size_bytes, 1_519_521_155);
        assert_eq!(
            distil.sha256,
            "ec2498919b498c5f6b00041adb45650124b3cd9f26f545fffa8f5d11c28dcf26"
        );
    }

    #[test]
    fn managed_model_ids_and_filenames_are_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut filenames = std::collections::HashSet::new();
        for model in MODELS {
            assert!(ids.insert(model.id), "duplicate model id: {}", model.id);
            assert!(
                filenames.insert(model.filename),
                "duplicate model filename: {}",
                model.filename
            );
        }
    }

    #[test]
    fn checksum_rejection_removes_temp_and_preserves_installed_model() {
        let dir = tempdir().expect("temp dir");
        let destination = dir.path().join("model.bin");
        fs::write(&destination, b"known-good").expect("seed model");

        let result = write_verified_stream(
            Cursor::new(b"corrupt download"),
            &destination,
            &"0".repeat(64),
            &AtomicBool::new(false),
            |_| {},
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read(&destination).expect("installed model"),
            b"known-good"
        );
        assert_eq!(fs::read_dir(dir.path()).expect("dir entries").count(), 1);
    }

    #[test]
    fn verified_download_atomically_replaces_the_target() {
        let dir = tempdir().expect("temp dir");
        let destination = dir.path().join("model.bin");
        fs::write(&destination, b"old model").expect("seed model");
        let replacement = b"complete replacement";
        let expected = format!("{:x}", Sha256::digest(replacement));

        write_verified_stream(
            Cursor::new(replacement),
            &destination,
            &expected,
            &AtomicBool::new(false),
            |_| {},
        )
        .expect("verified replacement");

        assert_eq!(
            fs::read(&destination).expect("installed model"),
            replacement
        );
        assert_eq!(fs::read_dir(dir.path()).expect("dir entries").count(), 1);
    }
}
