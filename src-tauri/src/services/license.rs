//! Licence storage and entitlement resolution.
//!
//! The licence *format* lives in the standalone `atmospeak-license` crate so it
//! can be tested and minted on any host. This module is the application-side
//! adapter: it reads the key from the OS keyring, resolves entitlements against
//! the build's release date, and shapes a status payload for the UI.
//!
//! Two invariants hold everywhere in this file:
//!
//! 1. **No network access.** Activation is a local signature check. There is no
//!    activation server to be unreachable, so activation cannot fail because a
//!    machine is offline. This is a headline product claim; do not break it.
//! 2. **Never break dictation.** Every failure path — absent key, unreadable
//!    keyring, forged signature, out-of-window build — resolves to
//!    [`Entitlements::free`]. Nothing in the core dictation path consults this
//!    module at all.

use anyhow::{Context, Result, bail};
use atmospeak_license::{Entitlements, Feature, License, LicenseTier, verify};
use chrono::NaiveDate;

use crate::models::LicenseStatus;

/// Shared with the polish API key so that Atmospeak owns one keyring service
/// name rather than several.
const KEYRING_SERVICE: &str = "atmospeak";
const KEYRING_USER: &str = "license-key";

/// Stamped by `build.rs`. Always a valid `YYYY-MM-DD`.
const RELEASE_DATE: &str = env!("ATMOSPEAK_RELEASE_DATE");

/// Every gateable capability, in the order the UI presents them.
const ALL_FEATURES: &[(Feature, &str)] = &[
    (Feature::CompliancePack, "compliancePack"),
    (Feature::VoiceMacros, "voiceMacros"),
    (Feature::McpServer, "mcpServer"),
    (Feature::IdeAwareness, "ideAwareness"),
    (Feature::Sync, "sync"),
    (Feature::TeamSharedVocabulary, "teamSharedVocabulary"),
];

/// The resolved licence picture for this process.
#[derive(Debug, Clone)]
pub struct LicenseState {
    pub entitlements: Entitlements,
    pub license: Option<License>,
}

impl LicenseState {
    pub fn free() -> Self {
        Self {
            entitlements: Entitlements::free(),
            license: None,
        }
    }
}

impl Default for LicenseState {
    fn default() -> Self {
        Self::free()
    }
}

/// The date this build was released, used instead of the wall clock so a wrong
/// system clock can never revoke a paid feature.
pub fn build_release_date() -> NaiveDate {
    // `build.rs` validates the format, so this parse cannot realistically fail.
    // If it somehow did, fall back to the epoch: that keeps every licence in
    // window, because a bug of ours must never lock out someone who paid.
    NaiveDate::parse_from_str(RELEASE_DATE, "%Y-%m-%d")
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch is a valid date"))
}

/// Resolve the licence stored on this machine. Never fails, never blocks, never
/// touches the network.
pub fn load() -> LicenseState {
    let Some(key) = read_keyring_license() else {
        return LicenseState::free();
    };

    match verify(&key) {
        Ok(license) => LicenseState {
            entitlements: Entitlements::for_license(&license, build_release_date()),
            license: Some(license),
        },
        // A key that no longer verifies — a rotated signing key, a corrupted
        // credential store — degrades silently to free rather than erroring at
        // people mid-sentence. The activation panel reports the real reason.
        Err(_) => LicenseState::free(),
    }
}

/// Verify and persist a licence key. The key is only written once it has
/// verified, so a rejected key never displaces a working one.
pub fn activate(key: &str) -> Result<LicenseState> {
    let license = verify(key).map_err(|error| anyhow::anyhow!("{error}"))?;
    set_keyring_license(key)?;

    Ok(LicenseState {
        entitlements: Entitlements::for_license(&license, build_release_date()),
        license: Some(license),
    })
}

/// Remove the licence from this machine, for example before selling the
/// hardware on. The licence key itself remains valid and can be re-entered.
pub fn deactivate() -> Result<LicenseState> {
    clear_keyring_license()?;
    Ok(LicenseState::free())
}

pub fn status(state: &LicenseState) -> LicenseStatus {
    let features = ALL_FEATURES
        .iter()
        .filter(|(feature, _)| state.entitlements.allows(*feature))
        .map(|(_, id)| (*id).to_string())
        .collect();

    LicenseStatus {
        tier: state.entitlements.tier().as_str().to_string(),
        activated: state.license.is_some(),
        in_update_window: state.entitlements.in_update_window(),
        // Rendered as a string: licence ids exceed JavaScript's exact integer
        // range and must never be silently rounded in a support conversation.
        license_id: state
            .license
            .as_ref()
            .map(|license| license.license_id.to_string()),
        issued_at: state.license.as_ref().map(|license| license.issued_at),
        updates_until: state.license.as_ref().map(|license| license.updates_until),
        seats: state.license.as_ref().map_or(0, |license| license.seats),
        build_released_on: build_release_date(),
        features,
    }
}

pub fn read_keyring_license() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    entry
        .get_password()
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn set_keyring_license(key: &str) -> Result<()> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        bail!("licence key cannot be empty");
    }
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).context("failed to open OS keyring")?;
    entry
        .set_password(trimmed)
        .context("failed to store the licence key in the OS keyring")?;
    Ok(())
}

fn clear_keyring_license() -> Result<()> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).context("failed to open OS keyring")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("failed to clear the licence key from the OS keyring"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_release_date_parses() {
        // Guards the build.rs contract. If this fails, every licence silently
        // falls back to the epoch.
        assert!(NaiveDate::parse_from_str(RELEASE_DATE, "%Y-%m-%d").is_ok());
    }

    #[test]
    fn free_state_reports_no_features() {
        let status = status(&LicenseState::free());

        assert_eq!(status.tier, "free");
        assert!(!status.activated);
        assert!(!status.in_update_window);
        assert!(status.features.is_empty());
        assert_eq!(status.seats, 0);
        assert!(status.license_id.is_none());
    }

    #[test]
    fn status_lists_the_features_a_pro_licence_unlocks() {
        let license = License {
            key_id: 1,
            license_id: 42,
            tier: LicenseTier::Pro,
            email_hash: [0u8; 8],
            issued_at: build_release_date(),
            updates_until: build_release_date(),
            seats: 1,
        };
        let state = LicenseState {
            entitlements: Entitlements::for_license(&license, build_release_date()),
            license: Some(license),
        };

        let status = status(&state);
        assert_eq!(status.tier, "pro");
        assert!(status.activated);
        assert!(status.in_update_window);
        assert!(status.features.contains(&"mcpServer".to_string()));
        // Team capability stays locked on a Pro licence.
        assert!(
            !status
                .features
                .contains(&"teamSharedVocabulary".to_string())
        );
    }

    #[test]
    fn an_out_of_window_build_reports_the_tier_but_no_features() {
        let issued = NaiveDate::from_ymd_opt(2020, 1, 1).expect("valid date");
        let license = License {
            key_id: 1,
            license_id: 42,
            tier: LicenseTier::Pro,
            email_hash: [0u8; 8],
            issued_at: issued,
            updates_until: issued,
            seats: 1,
        };
        let state = LicenseState {
            entitlements: Entitlements::for_license(&license, build_release_date()),
            license: Some(license),
        };

        let status = status(&state);
        // The purchase is still recognised — the UI says "renew for updates",
        // never "your licence is invalid".
        assert!(status.activated);
        assert_eq!(status.tier, "pro");
        assert!(!status.in_update_window);
        assert!(status.features.is_empty());
    }

    #[test]
    fn activation_rejects_a_key_that_is_not_signed() {
        // Must not touch the keyring on the failure path.
        assert!(activate("ATMO-NOTAREALKEY").is_err());
        assert!(activate("").is_err());
    }
}
