use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use uuid::Uuid;

use crate::{models::ModelInventoryItem, services::app_state::AppState};

pub use atmospeak_asr_protocol::{PARAKEET_MODEL_FILE as BUNDLED_MODEL_FILE, PARAKEET_MODEL_ID as BUNDLED_MODEL_ID};
use atmospeak_asr_protocol::PARAKEET_MODEL_BYTES;

pub const BUNDLED_MODEL_LABEL: &str = "Parakeet Unified English 0.6B";

#[derive(Debug, Clone, Copy)]
pub struct ModelDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub relative_path: &'static str,
    pub bundled: bool,
}

pub const MODELS: &[ModelDescriptor] = &[ModelDescriptor {
    id: BUNDLED_MODEL_ID,
    label: BUNDLED_MODEL_LABEL,
    relative_path: BUNDLED_MODEL_FILE,
    bundled: true,
}];

pub fn descriptor(model_id: &str) -> Option<&'static ModelDescriptor> {
    MODELS.iter().find(|model| model.id == model_id)
}

pub fn models_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("models")
}

pub fn installed_model_path(app_dir: &Path, model: &ModelDescriptor) -> PathBuf {
    models_dir(app_dir).join(model.relative_path)
}

pub fn is_model_file_ready(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| {
        metadata.is_file() && metadata.len() == PARAKEET_MODEL_BYTES
    })
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
    let installed = path
        .as_ref()
        .is_some_and(|candidate| is_model_file_ready(candidate));

    ModelInventoryItem {
        id: model.id.to_string(),
        label: model.label.to_string(),
        installed,
        bundled: model.bundled,
        path: installed.then(|| {
            path.expect("installed model path")
                .to_string_lossy()
                .to_string()
        }),
        size_mb: Some(PARAKEET_MODEL_BYTES / (1024 * 1024)),
    }
}

pub fn download(_app: &AppHandle, model_id: &str) -> Result<()> {
    let model = descriptor(model_id).ok_or_else(|| anyhow!("unknown model: {model_id}"))?;
    if model.bundled {
        bail!(
            "{} is bundled and does not need to be downloaded",
            model.label
        );
    }
    bail!("no downloadable ASR models are configured")
}

pub fn cancel(state: &AppState) -> bool {
    state.cancel_model_download()
}

pub fn delete(_app: &AppHandle, model_id: &str) -> Result<()> {
    let model = descriptor(model_id).ok_or_else(|| anyhow!("unknown model: {model_id}"))?;
    if model.bundled {
        bail!("the bundled Parakeet model cannot be deleted");
    }
    bail!("no deletable ASR models are configured")
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
        let model = descriptor(BUNDLED_MODEL_ID).expect("Parakeet model");
        assert_eq!(
            installed_model_path(root, model),
            root.join("models").join(BUNDLED_MODEL_FILE)
        );
    }

    #[test]
    fn bundled_model_preserves_the_pinned_parakeet_filename() {
        let model = descriptor(BUNDLED_MODEL_ID).expect("Parakeet model");
        assert_eq!(model.relative_path, BUNDLED_MODEL_FILE);
        assert!(model.bundled);
    }

    #[test]
    fn managed_model_ids_and_filenames_are_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut filenames = std::collections::HashSet::new();
        for model in MODELS {
            assert!(ids.insert(model.id), "duplicate model id: {}", model.id);
            assert!(
                filenames.insert(model.relative_path),
                "duplicate model file: {}",
                model.relative_path
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
