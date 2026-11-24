use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager, path::BaseDirectory};

use crate::models::{AppSettings, ModelInventory, ModelInventoryItem, ModelStatus, RuntimeSource};
use crate::services::{app_state::AppState, model_downloader};

#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    pub source: RuntimeSource,
    pub executable_path: PathBuf,
    pub model_path: PathBuf,
}

pub fn resolve_runtime(app: &AppHandle, settings: &AppSettings) -> Result<ResolvedRuntime> {
    if settings.advanced_runtime_enabled && cfg!(debug_assertions) {
        return Ok(ResolvedRuntime {
            source: RuntimeSource::AdvancedOverride,
            executable_path: validate_advanced_executable(
                app,
                settings.advanced_asr_executable_path.trim(),
            )?,
            model_path: validate_advanced_model_path(app, settings.advanced_model_path.trim())?,
        });
    }

    Ok(ResolvedRuntime {
        source: RuntimeSource::Bundled,
        executable_path: resolve_executable(app)?,
        model_path: resolve_bundled_model(app)?,
    })
}

fn validate_advanced_executable(app: &AppHandle, raw: &str) -> Result<PathBuf> {
    if raw.is_empty() {
        return Err(anyhow!("advanced ASR executable path is empty"));
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(anyhow!("advanced ASR executable path must be absolute"));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| anyhow!("advanced ASR executable path is invalid: {error}"))?;
    if !canonical.is_file() {
        return Err(anyhow!("advanced ASR executable path is not a file"));
    }
    ensure_trusted_path(app, &canonical)?;
    validate_runtime_dependencies(&canonical)?;
    Ok(canonical)
}

const ASR_DEPENDENCIES: &[&str] = &[
    "transcribe.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu-x64.dll",
    "MSVCP140.dll", "MSVCP140_1.dll", "VCRUNTIME140.dll", "VCRUNTIME140_1.dll",
];

fn validate_runtime_dependencies(executable: &Path) -> Result<()> {
    let directory = executable
        .parent()
        .ok_or_else(|| anyhow!("ASR executable has no parent directory"))?;
    for dependency in ASR_DEPENDENCIES {
        let path = directory.join(dependency);
        if !path.is_file() {
            return Err(anyhow!(
                "ASR runtime dependency is missing beside executable: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_advanced_model_path(app: &AppHandle, raw: &str) -> Result<PathBuf> {
    if raw.is_empty() {
        return Err(anyhow!("advanced model path is empty"));
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(anyhow!("advanced model path must be absolute"));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| anyhow!("advanced model path is invalid: {error}"))?;
    if !model_downloader::is_model_file_ready(&canonical) {
        return Err(anyhow!(
            "advanced model path must point to the complete Parakeet GGUF"
        ));
    }
    ensure_trusted_path(app, &canonical)?;
    Ok(canonical)
}

fn ensure_trusted_path(app: &AppHandle, path: &Path) -> Result<()> {
    let trusted_roots = trusted_runtime_roots(app);
    let allowed = trusted_roots.iter().any(|root| path.starts_with(root));
    if !allowed {
        return Err(anyhow!(
            "advanced override path must be under the app resource or managed data directory"
        ));
    }
    Ok(())
}

fn trusted_runtime_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(resource) = app.path().resource_dir() {
        if let Ok(canonical) = resource.canonicalize() {
            roots.push(canonical);
        } else {
            roots.push(resource);
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Ok(canonical) = manifest.canonicalize() {
        roots.push(canonical);
    } else {
        roots.push(manifest);
    }
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(canonical) = state.app_dir.canonicalize() {
            roots.push(canonical);
        } else {
            roots.push(state.app_dir.clone());
        }
    }
    roots
}

pub fn model_status(app: &AppHandle, settings: &AppSettings) -> ModelStatus {
    match resolve_runtime(app, settings) {
        Ok(runtime) => {
            let executable_found = runtime.executable_path.is_file();
            let model_found = model_downloader::is_model_file_ready(&runtime.model_path);
            let ready = executable_found && model_found;
            let source_label = match runtime.source {
                RuntimeSource::Bundled => "bundled",
                RuntimeSource::ManagedModel => "managed",
                RuntimeSource::AdvancedOverride => "advanced",
            };
            let message = match (ready, executable_found, model_found, &runtime.source) {
                (true, _, _, RuntimeSource::Bundled) => {
                    "Bundled Parakeet runtime is ready.".to_string()
                }
                (true, _, _, RuntimeSource::ManagedModel) => {
                    "Managed Parakeet runtime is ready.".to_string()
                }
                (true, _, _, RuntimeSource::AdvancedOverride) => {
                    "Advanced transcription runtime override is ready.".to_string()
                }
                (false, false, true, _) => {
                    format!("The {source_label} ASR executable could not be found.")
                }
                (false, true, false, _) => {
                    format!("The {source_label} Parakeet GGUF is missing or has the wrong size.")
                }
                _ => format!("The {source_label} transcription runtime is incomplete."),
            };

            ModelStatus {
                executable_found,
                model_found,
                ready,
                message,
                source: runtime.source,
                executable_path: runtime.executable_path.to_string_lossy().to_string(),
                model_path: runtime.model_path.to_string_lossy().to_string(),
            }
        }
        Err(error) => ModelStatus {
            executable_found: false,
            model_found: false,
            ready: false,
            message: error.to_string(),
            source: if settings.advanced_runtime_enabled {
                RuntimeSource::AdvancedOverride
            } else {
                RuntimeSource::Bundled
            },
            executable_path: String::new(),
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
    ModelInventory {
        active_model_id: if settings.advanced_runtime_enabled {
            "advanced-override".to_string()
        } else {
            model_downloader::BUNDLED_MODEL_ID.to_string()
        },
        models,
    }
}

fn resolve_bundled_model(app: &AppHandle) -> Result<PathBuf> {
    let filename = model_downloader::BUNDLED_MODEL_FILE;
    let path = resolve_first_existing(
        app,
        &[&format!("resources/models/{filename}"), &format!("models/{filename}")],
    )?;
    if model_downloader::is_model_file_ready(&path) {
        Ok(path)
    } else {
        Err(anyhow!(
            "Bundled Parakeet GGUF is missing or has the wrong size."
        ))
    }
}

fn resolve_first_existing(app: &AppHandle, candidates: &[&str]) -> Result<PathBuf> {
    let resolved = candidates
        .iter()
        .filter_map(|candidate| resolve_resource(app, candidate).ok())
        .find(|path| path.exists());

    resolved.ok_or_else(|| anyhow!("Bundled runtime resource is missing from this build."))
}

fn resolve_executable(app: &AppHandle) -> Result<PathBuf> {
    let path = resolve_first_existing(
        app,
        &[
            "resources/asr/atmospeak-asr-cpu.exe",
            "asr/atmospeak-asr-cpu.exe",
        ],
    )?;
    if path.is_file() {
        validate_runtime_dependencies(&path)?;
        Ok(path)
    } else {
        Err(anyhow!(
            "Bundled ASR executable is not a file: {}",
            path.display()
        ))
    }
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
        assert!(settings.advanced_asr_executable_path.is_empty());
    }

    #[test]
    fn model_file_rejects_missing_or_incomplete_weights() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join(model_downloader::BUNDLED_MODEL_FILE);
        assert!(!model_downloader::is_model_file_ready(&path));
        std::fs::write(&path, b"incomplete").expect("truncated model");
        assert!(!model_downloader::is_model_file_ready(&path));
    }

    #[test]
    fn runtime_requires_adjacent_parakeet_dependencies() {
        let directory = tempfile::tempdir().expect("temp dir");
        let executable = directory.path().join("atmospeak-asr-cpu.exe");
        std::fs::write(&executable, b"executable").expect("executable");
        assert!(validate_runtime_dependencies(&executable).is_err());

        for dependency in ASR_DEPENDENCIES {
            std::fs::write(directory.path().join(dependency), b"runtime").expect("runtime DLL");
        }
        assert!(validate_runtime_dependencies(&executable).is_ok());
    }
}
