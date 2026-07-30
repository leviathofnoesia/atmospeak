//! Append-only outbound network ledger for Atmospeak Pro compliance exports.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkLedgerError {
    #[error("failed to access network ledger: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse ledger entry: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("network ledger lock poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub at: DateTime<Utc>,
    /// Short category: `update_check`, `model_download`, `polish_remote`, `licence_validate`, …
    pub kind: String,
    /// Host or URL origin involved (never include secrets).
    pub target: String,
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

static APPEND_LOCK: Mutex<()> = Mutex::new(());

pub struct NetworkLedger {
    path: PathBuf,
}

impl NetworkLedger {
    pub fn path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("pro").join("network_ledger.jsonl")
    }

    pub fn open(app_data_dir: &Path) -> Result<Self, NetworkLedgerError> {
        let path = Self::path(app_data_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // create(true)+append(true) never truncates an existing ledger.
        let _ = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { path })
    }

    pub fn append(&self, entry: &LedgerEntry) -> Result<(), NetworkLedgerError> {
        let _guard = APPEND_LOCK
            .lock()
            .map_err(|_| NetworkLedgerError::LockPoisoned)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        // If a prior crash left a partial line, start the next record on a new line.
        let len = file.metadata()?.len();
        if len > 0 {
            let raw = fs::read(&self.path)?;
            if raw.last().copied() != Some(b'\n') {
                file.write_all(b"\n")?;
            }
        }
        writeln!(file, "{}", serde_json::to_string(entry)?)?;
        file.flush()?;
        Ok(())
    }

    pub fn record(
        &self,
        kind: impl Into<String>,
        target: impl Into<String>,
        allowed: bool,
        detail: Option<String>,
    ) -> Result<LedgerEntry, NetworkLedgerError> {
        let entry = LedgerEntry {
            at: Utc::now(),
            kind: kind.into(),
            target: target.into(),
            allowed,
            detail,
        };
        self.append(&entry)?;
        Ok(entry)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<LedgerEntry>, NetworkLedgerError> {
        let raw = fs::read_to_string(&self.path)?;
        let mut entries = Vec::new();
        for line in raw.lines().rev() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Skip corrupt / truncated lines from a mid-write crash.
            let Ok(entry) = serde_json::from_str(line) else {
                continue;
            };
            entries.push(entry);
            if entries.len() >= limit {
                break;
            }
        }
        entries.reverse();
        Ok(entries)
    }

    pub fn export_jsonl(&self) -> Result<String, NetworkLedgerError> {
        Ok(fs::read_to_string(&self.path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_list() {
        let dir = tempdir().unwrap();
        let ledger = NetworkLedger::open(dir.path()).unwrap();
        ledger
            .record("update_check", "updates.novpax.org", true, None)
            .unwrap();
        ledger
            .record("model_download", "huggingface.co", false, Some("airplane".into()))
            .unwrap();
        let recent = ledger.list_recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert!(!recent[1].allowed);
    }

    #[test]
    fn open_does_not_truncate() {
        let dir = tempdir().unwrap();
        let ledger = NetworkLedger::open(dir.path()).unwrap();
        ledger
            .record("update_check", "updates.novpax.org", true, None)
            .unwrap();
        let again = NetworkLedger::open(dir.path()).unwrap();
        assert_eq!(again.list_recent(10).unwrap().len(), 1);
    }

    #[test]
    fn skips_truncated_tail_and_recovers() {
        let dir = tempdir().unwrap();
        let path = NetworkLedger::path(dir.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{\"at\":\"2020-01-01T00:00:00Z\",\"kind\":\"a\",\"target\":\"t\",\"allowed\":true}\n{\"partial").unwrap();
        let ledger = NetworkLedger::open(dir.path()).unwrap();
        ledger
            .record("update_check", "updates.novpax.org", true, None)
            .unwrap();
        let recent = ledger.list_recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].kind, "a");
        assert_eq!(recent[1].kind, "update_check");
    }
}
