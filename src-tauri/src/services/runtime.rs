use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager, path::BaseDirectory};

use crate::models::{AppSettings, ModelInventory, ModelInventoryItem, ModelStatus, RuntimeSource};
use crate::services::{app_state::AppState, model_downloader};

#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    pub source: RuntimeSource,
    pub whisper_cli_path: PathBuf,
    pub model_path: PathBuf,
}

/// The resident Phase B server, resolved next to the CLI binary.
///
/// Advanced overrides only name a `whisper-cli.exe`, so for that source we look for
/// a sibling `whisper-server.exe`. Absent one, the caller falls back to the CLI.
pub fn resolve_server(app: &AppHandle, settings: &AppSettings) -> Option<PathBuf> {
    let runtime = resolve_runtime(app, settings).ok()?;
    let candidate = runtime
        .whisper_cli_path
        .parent()?
        .join("whisper-server.exe");
    candidate.is_file().then_some(candidate)
}

pub fn resolve_runtime(app: &AppHandle, settings: &AppSettings) -> Result<ResolvedRuntime> {
    if settings.advanced_runtime_enabled {
        return Ok(ResolvedRuntime {
            source: RuntimeSource::AdvancedOverride,
            whisper_cli_path: PathBuf::from(settings.advanced_whisper_cli_path.trim()),
            model_path: PathBuf::from(settings.advanced_model_path.trim()),
        });
    }

    let bundled_model = resolve_bundled_model(app)?;
    let (source, model_path) = if let Some(state) = app.try_state::<AppState>() {
        resolve_selected_model(
            &state.app_dir,
            settings.active_model_id.trim(),
            bundled_model,
        )
    } else {
        (RuntimeSource::Bundled, bundled_model)
    };

    Ok(ResolvedRuntime {
        source,
        whisper_cli_path: resolve_first_existing(
            app,
            &[
                "resources/whisper-runtime/whisper-cli.exe",
                "whisper-runtime/whisper-cli.exe",
            ],
        )?,
        model_path,
    })
}

pub fn model_status(app: &AppHandle, settings: &AppSettings) -> ModelStatus {
    match resolve_runtime(app, settings) {
        Ok(runtime) => {
            let whisper_cli_found = runtime.whisper_cli_path.is_file();
            let model_found = runtime.model_path.is_file();
            let ready = whisper_cli_found && model_found;
            let source_label = match runtime.source {
                RuntimeSource::Bundled => "bundled",
                RuntimeSource::ManagedModel => "downloaded",
                RuntimeSource::AdvancedOverride => "advanced",
            };
            let message = match (ready, whisper_cli_found, model_found, &runtime.source) {
                (true, _, _, RuntimeSource::Bundled) => {
                    "Bundled offline transcription runtime is ready.".to_string()
                }
                (true, _, _, RuntimeSource::ManagedModel) => {
                    "Downloaded offline transcription model is ready.".to_string()
                }
                (true, _, _, RuntimeSource::AdvancedOverride) => {
                    "Advanced transcription runtime override is ready.".to_string()
                }
                (false, false, true, _) => {
                    format!("The {source_label} whisper-cli.exe could not be found.")
                }
                (false, true, false, _) => {
                    format!("The {source_label} speech model could not be found.")
                }
                _ => format!("The {source_label} transcription runtime is incomplete."),
            };

            ModelStatus {
                whisper_cli_found,
                model_found,
                ready,
                message,
                source: runtime.source,
                whisper_cli_path: runtime.whisper_cli_path.to_string_lossy().to_string(),
                model_path: runtime.model_path.to_string_lossy().to_string(),
            }
        }
        Err(error) => ModelStatus {
            whisper_cli_found: false,
            model_found: false,
            ready: false,
            message: error.to_string(),
            source: if settings.advanced_runtime_enabled {
                RuntimeSource::AdvancedOverride
            } else {
                RuntimeSource::Bundled
            },
            whisper_cli_path: String::new(),
            model_path: String::new(),
        },
    }
}

pub fn model_inventory(app: &AppHandle, settings: &AppSettings) -> ModelInventory {
    let bundled_path = resolve_bundled_model(app).ok();
    let app_dir = app
        .try_state::<AppState>()
        .map(|state| state.app_dir.clone())
        .unwrap_or_default();
    let models = model_downloader::MODELS
        .iter()
        .map(|model| model_downloader::inventory_item(&app_dir, model, bundled_path.as_deref()))
        .collect::<Vec<ModelInventoryItem>>();
    let selected_is_installed = model_downloader::descriptor(settings.active_model_id.trim())
        .and_then(|selected| models.iter().find(|model| model.id == selected.id))
        .is_some_and(|model| model.installed);

    ModelInventory {
        active_model_id: if settings.advanced_runtime_enabled {
            "advanced-override".to_string()
        } else if selected_is_installed {
            settings.active_model_id.clone()
        } else {
            model_downloader::BUNDLED_MODEL_ID.to_string()
        },
        models,
    }
}

fn resolve_bundled_model(app: &AppHandle) -> Result<PathBuf> {
    resolve_first_existing(
        app,
        &[
            "resources/models/ggml-base.en.bin",
            "models/ggml-base.en.bin",
        ],
    )
}

fn resolve_selected_model(
    app_dir: &Path,
    active_model_id: &str,
    bundled_model: PathBuf,
) -> (RuntimeSource, PathBuf) {
    let Some(model) = model_downloader::descriptor(active_model_id) else {
        return (RuntimeSource::Bundled, bundled_model);
    };
    if model.bundled {
        return (RuntimeSource::Bundled, bundled_model);
    }
    let managed_path = model_downloader::installed_model_path(app_dir, model);
    if managed_path.is_file() {
        (RuntimeSource::ManagedModel, managed_path)
    } else {
        (RuntimeSource::Bundled, bundled_model)
    }
}

fn resolve_first_existing(app: &AppHandle, candidates: &[&str]) -> Result<PathBuf> {
    let resolved = candidates
        .iter()
        .filter_map(|candidate| resolve_resource(app, candidate).ok())
        .find(|path| path.exists());

    resolved.ok_or_else(|| anyhow!("Bundled runtime resource is missing from this build."))
}

fn resolve_resource(app: &AppHandle, resource: &str) -> Result<PathBuf> {
    if let Ok(path) = app.path().resolve(resource, BaseDirectory::Resource) {
        return Ok(path);
    }

    let cwd_candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join(resource);
    if cwd_candidate.exists() {
        Ok(cwd_candidate)
    } else {
        Err(anyhow!("resource not found: {resource}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_overrides_are_disabled_by_default() {
        let settings = AppSettings::default();
        assert!(!settings.advanced_runtime_enabled);
        assert_eq!(settings.active_model_id, model_downloader::BUNDLED_MODEL_ID);
        assert!(settings.advanced_model_path.is_empty());
        assert!(settings.advanced_whisper_cli_path.is_empty());
    }

    #[test]
    fn missing_managed_model_falls_back_to_bundled_base() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bundled = dir.path().join("bundled-base.bin");
        std::fs::write(&bundled, b"base").expect("bundled model");

        let (source, selected) = resolve_selected_model(dir.path(), "small.en", bundled.clone());

        assert_eq!(source, RuntimeSource::Bundled);
        assert_eq!(selected, bundled);
    }

    #[test]
    fn installed_managed_model_is_selected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let model = model_downloader::descriptor("tiny.en").expect("tiny model");
        let managed = model_downloader::installed_model_path(dir.path(), model);
        std::fs::create_dir_all(managed.parent().expect("model parent")).expect("model dir");
        std::fs::write(&managed, b"tiny").expect("managed model");

        let (source, selected) =
            resolve_selected_model(dir.path(), "tiny.en", dir.path().join("base.bin"));

        assert_eq!(source, RuntimeSource::ManagedModel);
        assert_eq!(selected, managed);
    }
}
