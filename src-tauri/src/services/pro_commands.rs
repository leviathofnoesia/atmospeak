//! Pro-only host adapters: Polar licence, airplane mode, network ledger, gated updater.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::services::polar_license;

type CommandResult<T> = Result<T, String>;

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app data dir unavailable: {e}"))
}

fn hostname_label() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Atmospeak-Pro".into())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProFeatureStatus {
    pub airplane_mode: atmospeak_pro::AirplaneMode,
    pub ledger_recent: Vec<atmospeak_pro::LedgerEntry>,
    pub capabilities: Vec<CapabilityInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityInfo {
    pub id: &'static str,
    pub label: &'static str,
}

#[tauri::command]
pub fn get_license_status(app: AppHandle) -> CommandResult<polar_license::LicenceStatus> {
    let dir = app_data_dir(&app)?;
    Ok(polar_license::status(&dir))
}

#[tauri::command]
pub fn activate_license(app: AppHandle, key: String) -> CommandResult<polar_license::LicenceStatus> {
    let dir = app_data_dir(&app)?;
    polar_license::activate(&dir, &key, &hostname_label())
}

#[tauri::command]
pub fn deactivate_license(app: AppHandle) -> CommandResult<polar_license::LicenceStatus> {
    let dir = app_data_dir(&app)?;
    polar_license::deactivate(&dir)
}

#[tauri::command]
pub fn validate_license(app: AppHandle) -> CommandResult<polar_license::LicenceStatus> {
    let dir = app_data_dir(&app)?;
    polar_license::validate_online(&dir)
}

#[tauri::command]
pub fn get_pro_feature_status(app: AppHandle) -> CommandResult<ProFeatureStatus> {
    let dir = app_data_dir(&app)?;
    if !polar_license::entitlements_ok(&dir) {
        return Err("activate a valid Pro licence to use Pro features".into());
    }
    let airplane_mode = atmospeak_pro::AirplaneMode::load(&dir).map_err(|e| e.to_string())?;
    let ledger = atmospeak_pro::NetworkLedger::open(&dir).map_err(|e| e.to_string())?;
    let ledger_recent = ledger.list_recent(50).map_err(|e| e.to_string())?;
    let capabilities = atmospeak_pro::ProCapability::ALL
        .iter()
        .map(|c| CapabilityInfo {
            id: c.id(),
            label: c.label(),
        })
        .collect();
    Ok(ProFeatureStatus {
        airplane_mode,
        ledger_recent,
        capabilities,
    })
}

#[tauri::command]
pub fn set_airplane_mode(
    app: AppHandle,
    enabled: bool,
) -> CommandResult<atmospeak_pro::AirplaneMode> {
    let dir = app_data_dir(&app)?;
    if !polar_license::entitlements_ok(&dir) {
        return Err("activate a valid Pro licence to use airplane mode".into());
    }
    let state =
        atmospeak_pro::AirplaneMode::set_enabled(&dir, enabled).map_err(|e| e.to_string())?;
    let ledger = atmospeak_pro::NetworkLedger::open(&dir).map_err(|e| e.to_string())?;
    let _ = ledger.record(
        "airplane_mode_toggle",
        "local",
        true,
        Some(format!("enabled={enabled}")),
    );
    Ok(state)
}

#[tauri::command]
pub fn export_network_ledger(app: AppHandle) -> CommandResult<String> {
    let dir = app_data_dir(&app)?;
    if !polar_license::entitlements_ok(&dir) {
        return Err("activate a valid Pro licence to export the network ledger".into());
    }
    let ledger = atmospeak_pro::NetworkLedger::open(&dir).map_err(|e| e.to_string())?;
    ledger.export_jsonl().map_err(|e| e.to_string())
}

/// Whether outbound network work is allowed under Pro airplane mode.
pub fn outbound_allowed(app: &AppHandle) -> bool {
    let Ok(dir) = app_data_dir(app) else {
        return true;
    };
    match atmospeak_pro::AirplaneMode::load(&dir) {
        Ok(state) => state.allows_outbound(),
        Err(_) => true,
    }
}

pub fn record_outbound(
    app: &AppHandle,
    kind: &str,
    target: &str,
    allowed: bool,
    detail: Option<String>,
) {
    let Ok(dir) = app_data_dir(app) else {
        return;
    };
    if let Ok(ledger) = atmospeak_pro::NetworkLedger::open(&dir) {
        let _ = ledger.record(kind, target, allowed, detail);
    }
}

#[tauri::command]
pub async fn check_pro_update(app: AppHandle) -> CommandResult<serde_json::Value> {
    let dir = app_data_dir(&app)?;
    if !polar_license::entitlements_ok(&dir) {
        return Err("activate a valid Pro licence before checking updates".into());
    }
    if !outbound_allowed(&app) {
        record_outbound(
            &app,
            "update_check",
            "updates.novpax.org",
            false,
            Some("airplane_mode".into()),
        );
        return Err("airplane mode is on — outbound update checks are blocked".into());
    }
    let (key, activation) = polar_license::updater_auth_headers()?;
    let updater = app
        .updater_builder()
        .header("X-Atmospeak-License", key)
        .map_err(|e| e.to_string())?
        .header("X-Atmospeak-Activation", activation)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(update)) => {
            record_outbound(&app, "update_check", "updates.novpax.org", true, None);
            Ok(serde_json::json!({
                "available": true,
                "version": update.version,
                "body": update.body,
                "date": update.date.map(|d| d.to_string()),
            }))
        }
        Ok(None) => {
            record_outbound(
                &app,
                "update_check",
                "updates.novpax.org",
                true,
                Some("up_to_date".into()),
            );
            Ok(serde_json::json!({ "available": false }))
        }
        Err(err) => {
            record_outbound(
                &app,
                "update_check",
                "updates.novpax.org",
                false,
                Some(err.to_string()),
            );
            Err(err.to_string())
        }
    }
}
