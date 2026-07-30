//! Attested airplane mode for Atmospeak Pro.
//!
//! When enabled, the host must refuse new outbound network work on the
//! dictation / model-download / polish-remote paths. This module owns the
//! persisted flag and the policy check — socket enforcement lives in the app.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AirplaneModeError {
    #[error("failed to read airplane-mode state: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse airplane-mode state: {0}")]
    Parse(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AirplaneMode {
    pub enabled: bool,
    /// ISO-8601 timestamp of the last toggle (for compliance export).
    pub updated_at: String,
}

impl Default for AirplaneMode {
    fn default() -> Self {
        Self {
            enabled: false,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl AirplaneMode {
    pub fn state_path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("pro").join("airplane_mode.json")
    }

    pub fn load(app_data_dir: &Path) -> Result<Self, AirplaneModeError> {
        let path = Self::state_path(app_data_dir);
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self, app_data_dir: &Path) -> Result<(), AirplaneModeError> {
        let path = Self::state_path(app_data_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn set_enabled(app_data_dir: &Path, enabled: bool) -> Result<Self, AirplaneModeError> {
        let state = Self {
            enabled,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        state.save(app_data_dir)?;
        Ok(state)
    }

    /// Policy gate used by the host before any new outbound request.
    pub fn allows_outbound(&self) -> bool {
        !self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_allows_outbound() {
        assert!(AirplaneMode::default().allows_outbound());
    }

    #[test]
    fn enabled_blocks_outbound() {
        let dir = tempdir().unwrap();
        let state = AirplaneMode::set_enabled(dir.path(), true).unwrap();
        assert!(!state.allows_outbound());
        let loaded = AirplaneMode::load(dir.path()).unwrap();
        assert!(loaded.enabled);
    }
}
