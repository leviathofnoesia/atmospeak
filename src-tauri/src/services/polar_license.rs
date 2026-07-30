//! Polar online licence activation / validation for Atmospeak Pro builds.
//!
//! Free builds omit this module (`cfg(feature = "pro")`). Licence secrets stay
//! in the OS keyring; Polar is contacted only from Pro.

use chrono::{DateTime, Duration, Utc};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SERVICE: &str = "atmospeak-pro";
const KEY_USER: &str = "polar-license-key";
const ACTIVATION_USER: &str = "polar-activation-id";
const STATE_FILE: &str = "licence_state.json";

const DEFAULT_GRACE_DAYS: i64 = 14;
const POLAR_VALIDATE_URL: &str = "https://api.polar.sh/v1/customer-portal/license-keys/validate";
const POLAR_ACTIVATE_URL: &str = "https://api.polar.sh/v1/customer-portal/license-keys/activate";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LicenceStatus {
    pub is_pro_build: bool,
    pub activated: bool,
    pub license_display: Option<String>,
    pub activation_id: Option<String>,
    pub valid: bool,
    pub offline_grace: bool,
    pub last_validated_at: Option<String>,
    pub updates_until: Option<String>,
    pub grace_days: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
    last_validated_at: DateTime<Utc>,
    updates_until: Option<String>,
    benefit_id: Option<String>,
    /// SHA-256 hex of the activated licence key — binds offline grace to that key.
    license_key_hash: String,
    /// Activation id written together with a successful Polar activate/validate.
    activation_id: String,
}

fn hash_license_key(key: &str) -> String {
    let digest = Sha256::digest(key.trim().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Production Nov Pax Polar org (Atmospeak Pro License). Not overridable in release builds.
const POLAR_ORGANIZATION_ID: &str = "97f0d813-d25f-4cc4-b934-fd4705a01c47";
/// Production Atmospeak Pro License Keys benefit. Not overridable in release builds.
const POLAR_LICENSE_BENEFIT_ID: &str = "b4e88474-01fa-450c-9aac-07bd92d8e887";

fn organization_id() -> Result<String, String> {
    // Env override is debug-only; release builds must not let users retarget Polar trust anchors.
    if cfg!(debug_assertions) {
        if let Ok(value) = std::env::var("ATMOSPEAK_POLAR_ORGANIZATION_ID") {
            if !value.trim().is_empty() {
                return Ok(value);
            }
        }
    }
    Ok(POLAR_ORGANIZATION_ID.to_string())
}

fn grace_days() -> i64 {
    std::env::var("ATMOSPEAK_LICENSE_GRACE_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_GRACE_DAYS)
}

fn keyring_entry(user: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, user).map_err(|e| format!("keyring unavailable: {e}"))
}

fn state_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("pro").join(STATE_FILE)
}

fn load_state(app_data_dir: &Path) -> Option<PersistedState> {
    let path = state_path(app_data_dir);
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_state(app_data_dir: &Path, state: &PersistedState) -> Result<(), String> {
    let path = state_path(app_data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
}

fn mask_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.len() <= 8 {
        return "****".to_string();
    }
    format!("****{}", &trimmed[trimmed.len().saturating_sub(6)..])
}

fn read_stored_key() -> Result<Option<String>, String> {
    match keyring_entry(KEY_USER)?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("failed to read licence key: {err}")),
    }
}

fn read_activation_id() -> Result<Option<String>, String> {
    match keyring_entry(ACTIVATION_USER)?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("failed to read activation id: {err}")),
    }
}

fn store_secrets(key: &str, activation_id: &str) -> Result<(), String> {
    let key_entry = keyring_entry(KEY_USER)?;
    let activation_entry = keyring_entry(ACTIVATION_USER)?;
    key_entry
        .set_password(key.trim())
        .map_err(|e| format!("failed to store licence key: {e}"))?;
    if let Err(err) = activation_entry.set_password(activation_id.trim()) {
        let _ = key_entry.delete_credential();
        return Err(format!("failed to store activation id: {err}"));
    }
    Ok(())
}

fn clear_secrets() -> Result<(), String> {
    if let Ok(entry) = keyring_entry(KEY_USER) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(err) => return Err(format!("failed to clear licence key: {err}")),
        }
    }
    if let Ok(entry) = keyring_entry(ACTIVATION_USER) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(err) => return Err(format!("failed to clear activation id: {err}")),
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct PolarLicenseKey {
    id: Option<String>,
    status: Option<String>,
    benefit_id: Option<String>,
    expires_at: Option<String>,
    #[serde(default)]
    activations: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct PolarActivationResponse {
    id: String,
    license_key: Option<PolarLicenseKey>,
}

#[derive(Debug, Deserialize)]
struct PolarValidateResponse {
    id: Option<String>,
    status: Option<String>,
    benefit_id: Option<String>,
    expires_at: Option<String>,
    activation: Option<PolarActivationBody>,
}

#[derive(Debug, Deserialize)]
struct PolarActivationBody {
    id: Option<String>,
}

fn expected_benefit_id() -> Option<String> {
    if cfg!(debug_assertions) {
        if let Ok(value) = std::env::var("ATMOSPEAK_POLAR_LICENSE_BENEFIT_ID") {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    Some(POLAR_LICENSE_BENEFIT_ID.to_string())
}

fn compute_updates_until(
    expires_at: Option<&str>,
    previous: Option<&PersistedState>,
) -> String {
    if let Some(expires) = expires_at.filter(|s| !s.is_empty()) {
        return expires.to_string();
    }
    if let Some(existing) = previous.and_then(|s| s.updates_until.as_deref()) {
        if !existing.is_empty() {
            return existing.to_string();
        }
    }
    // One-time Pro: 3 years of updates from first successful activation/validate.
    (Utc::now() + Duration::days(365 * 3)).to_rfc3339()
}

fn within_grace(last: DateTime<Utc>, days: i64) -> bool {
    Utc::now() <= last + Duration::days(days)
}

pub fn status(app_data_dir: &Path) -> LicenceStatus {
    let grace = grace_days();
    let key = read_stored_key().ok().flatten();
    let activation_id = read_activation_id().ok().flatten();
    let state = load_state(app_data_dir);

    let Some(key) = key else {
        return LicenceStatus {
            is_pro_build: true,
            activated: false,
            license_display: None,
            activation_id: None,
            valid: false,
            offline_grace: false,
            last_validated_at: None,
            updates_until: None,
            grace_days: grace,
            message: "Enter a Polar licence key to activate Atmospeak Pro.".into(),
        };
    };

    let Some(activation_id) = activation_id else {
        return LicenceStatus {
            is_pro_build: true,
            activated: false,
            license_display: Some(mask_key(&key)),
            activation_id: None,
            valid: false,
            offline_grace: false,
            last_validated_at: state.as_ref().map(|s| s.last_validated_at.to_rfc3339()),
            updates_until: state.and_then(|s| s.updates_until),
            grace_days: grace,
            message: "Licence key is present but not activated — run Activate.".into(),
        };
    };

    if let Some(state) = state.as_ref() {
        let key_matches = state.license_key_hash == hash_license_key(&key);
        let activation_matches = state.activation_id == activation_id;
        if key_matches && activation_matches && within_grace(state.last_validated_at, grace) {
            return LicenceStatus {
                is_pro_build: true,
                activated: true,
                license_display: Some(mask_key(&key)),
                activation_id: Some(activation_id),
                valid: true,
                offline_grace: true,
                last_validated_at: Some(state.last_validated_at.to_rfc3339()),
                updates_until: state.updates_until.clone(),
                grace_days: grace,
                message: format!(
                    "Licence valid (offline grace {grace} days from last online check)."
                ),
            };
        }
    }

    LicenceStatus {
        is_pro_build: true,
        activated: true,
        license_display: Some(mask_key(&key)),
        activation_id: Some(activation_id),
        valid: false,
        offline_grace: false,
        last_validated_at: state.as_ref().map(|s| s.last_validated_at.to_rfc3339()),
        updates_until: state.and_then(|s| s.updates_until),
        grace_days: grace,
        message: "Online validation required — grace period expired or state mismatch.".into(),
    }
}

pub fn deactivate(app_data_dir: &Path) -> Result<LicenceStatus, String> {
    clear_secrets()?;
    let path = state_path(app_data_dir);
    let _ = std::fs::remove_file(path);
    Ok(status(app_data_dir))
}

pub fn activate(app_data_dir: &Path, license_key: &str, device_label: &str) -> Result<LicenceStatus, String> {
    let organization_id = organization_id()?;
    let key = license_key.trim();
    if key.is_empty() {
        return Err("licence key is empty".into());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "key": key,
        "organization_id": organization_id,
        "label": device_label,
        "conditions": { "product": "atmospeak-pro" },
        "meta": { "app": "atmospeak-pro" }
    });

    let response = client
        .post(POLAR_ACTIVATE_URL)
        .json(&body)
        .send()
        .map_err(|e| format!("Polar activate request failed: {e}"))?;

    if !response.status().is_success() {
        let status_code = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Polar activate failed ({status_code}): {text}"));
    }

    let parsed: PolarActivationResponse = response
        .json()
        .map_err(|e| format!("Polar activate response invalid: {e}"))?;

    if let Some(expected) = expected_benefit_id() {
        let got = parsed
            .license_key
            .as_ref()
            .and_then(|k| k.benefit_id.as_deref())
            .unwrap_or("");
        if got != expected {
            return Err("licence key is not for the Atmospeak Pro benefit".into());
        }
    }

    let status_name = parsed
        .license_key
        .as_ref()
        .and_then(|k| k.status.as_deref())
        .unwrap_or("granted");
    if status_name != "granted" && status_name != "active" {
        return Err(format!("licence status is {status_name}, expected granted"));
    }

    let previous = load_state(app_data_dir);
    let updates_until = compute_updates_until(
        parsed
            .license_key
            .as_ref()
            .and_then(|k| k.expires_at.as_deref()),
        previous.as_ref(),
    );
    store_secrets(key, &parsed.id)?;
    save_state(
        app_data_dir,
        &PersistedState {
            last_validated_at: Utc::now(),
            updates_until: Some(updates_until),
            benefit_id: parsed.license_key.and_then(|k| k.benefit_id),
            license_key_hash: hash_license_key(key),
            activation_id: parsed.id.clone(),
        },
    )?;

    Ok(status(app_data_dir))
}

pub fn validate_online(app_data_dir: &Path) -> Result<LicenceStatus, String> {
    let organization_id = organization_id()?;
    let key = read_stored_key()?.ok_or_else(|| "no licence key stored".to_string())?;
    let activation_id = read_activation_id()?.ok_or_else(|| "no activation id stored".to_string())?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "key": key,
        "organization_id": organization_id,
        "activation_id": activation_id,
        "conditions": { "product": "atmospeak-pro" }
    });

    let response = client
        .post(POLAR_VALIDATE_URL)
        .json(&body)
        .send()
        .map_err(|e| format!("Polar validate request failed: {e}"))?;

    if !response.status().is_success() {
        let status_code = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Polar validate failed ({status_code}): {text}"));
    }

    let parsed: PolarValidateResponse = response
        .json()
        .map_err(|e| format!("Polar validate response invalid: {e}"))?;

    if let Some(expected) = expected_benefit_id() {
        let got = parsed.benefit_id.as_deref().unwrap_or("");
        if got != expected {
            return Err("licence key is not for the Atmospeak Pro benefit".into());
        }
    }

    let status_name = parsed.status.as_deref().unwrap_or("granted");
    if status_name != "granted" && status_name != "active" {
        return Err(format!("licence status is {status_name}"));
    }

    let previous = load_state(app_data_dir);
    let updates_until = compute_updates_until(parsed.expires_at.as_deref(), previous.as_ref());
    save_state(
        app_data_dir,
        &PersistedState {
            last_validated_at: Utc::now(),
            updates_until: Some(updates_until),
            benefit_id: parsed.benefit_id,
            license_key_hash: hash_license_key(&key),
            activation_id: activation_id.clone(),
        },
    )?;

    Ok(status(app_data_dir))
}

/// Credentials the gated Pro updater sends as request headers.
pub fn updater_auth_headers() -> Result<(String, String), String> {
    let key = read_stored_key()?.ok_or_else(|| "activate a licence before checking Pro updates".to_string())?;
    let activation = read_activation_id()?
        .ok_or_else(|| "activate a licence before checking Pro updates".to_string())?;
    Ok((key, activation))
}

pub fn entitlements_ok(app_data_dir: &Path) -> bool {
    let s = status(app_data_dir);
    s.valid
}
