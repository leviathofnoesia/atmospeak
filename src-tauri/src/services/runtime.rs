use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager, path::BaseDirectory};

use crate::models::{AppSettings, ModelInventory, ModelInventoryItem, ModelStatus, RuntimeSource};

const BUNDLED_MODEL_ID: &str = "base.en";
const BUNDLED_MODEL_SIZE_MB: u64 = 142;

#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    pub source: RuntimeSource,
    pub whisper_cli_path: PathBuf,
    pub model_path: PathBuf,
}

pub fn resolve_runtime(app: &AppHandle, settings: &AppSettings) -> Result<ResolvedRuntime> {
    if settings.advanced_runtime_enabled {
        return Ok(ResolvedRuntime {
            source: RuntimeSource::AdvancedOverride,
            whisper_cli_path: PathBuf::from(settings.advanced_whisper_cli_path.trim()),
            model_path: PathBuf::from(settings.advanced_model_path.trim()),
        });
    }

    Ok(ResolvedRuntime {
        source: RuntimeSource::Bundled,
        whisper_cli_path: resolve_first_existing(
            app,
            &[
                "resources/whisper-runtime/whisper-cli.exe",
                "whisper-runtime/whisper-cli.exe",
            ],
        )?,
        model_path: resolve_first_existing(
            app,
            &[
                "resources/models/ggml-base.en.bin",
                "models/ggml-base.en.bin",
            ],
        )?,
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
                RuntimeSource::AdvancedOverride => "advanced",
            };
            let message = match (ready, whisper_cli_found, model_found, &runtime.source) {
                (true, _, _, RuntimeSource::Bundled) => {
                    "Bundled offline transcription runtime is ready.".to_string()
                }
                (true, _, _, RuntimeSource::AdvancedOverride) => {
                    "Advanced transcription runtime override is ready.".to_string()
                }
                (false, false, true, _) => {
                    format!("The {source_label} whisper-cli.exe could not be found.")
                }
                (false, true, false, _) => {
                    format!("The {source_label} ggml-base.en.bin model could not be found.")
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
    let bundled = resolve_runtime(app, &AppSettings::default()).ok();
    let active = model_status(app, settings);
    let mut models = vec![ModelInventoryItem {
        id: BUNDLED_MODEL_ID.to_string(),
        label: "Base English".to_string(),
        installed: bundled
            .as_ref()
            .map(|runtime| runtime.model_path.is_file())
            .unwrap_or(false),
        bundled: true,
        path: bundled.map(|runtime| runtime.model_path.to_string_lossy().to_string()),
        size_mb: Some(BUNDLED_MODEL_SIZE_MB),
    }];

    for (id, label) in [
        ("tiny.en", "Tiny English"),
        ("small.en", "Small English"),
        ("medium.en", "Medium English"),
        ("distil-large-v3", "Distil Large v3"),
    ] {
        models.push(ModelInventoryItem {
            id: id.to_string(),
            label: label.to_string(),
            installed: false,
            bundled: false,
            path: None,
            size_mb: None,
        });
    }

    ModelInventory {
        active_model_id: if active.source == RuntimeSource::Bundled {
            BUNDLED_MODEL_ID.to_string()
        } else {
            "advanced-override".to_string()
        },
        models,
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
        assert!(settings.advanced_model_path.is_empty());
        assert!(settings.advanced_whisper_cli_path.is_empty());
    }
}
