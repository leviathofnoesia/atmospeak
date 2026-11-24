use crate::models::AppSettings;
use anyhow::{Result, bail};
use std::collections::HashSet;

fn application_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .trim_end_matches(".exe")
        .to_string()
}

pub fn validate(settings: &AppSettings) -> Result<()> {
    if settings.application_profiles.len() > 64 {
        bail!("Keep application profiles to 64 or fewer.");
    }
    let mut applications = HashSet::new();
    for profile in &settings.application_profiles {
        let key = application_key(&profile.application);
        if key.is_empty() || key.len() > 128 || profile.application.contains(['/', '\\']) {
            bail!("Use an application executable name, such as notepad.exe.");
        }
        if !applications.insert(key) {
            bail!("Each application can have only one profile.");
        }
        if profile.instructions.len() > 4000 {
            bail!("Application instructions must be 4,000 characters or fewer.");
        }
    }
    Ok(())
}

/// Resolve once at capture start; later focus or Settings changes cannot change
/// the transformation policy of an in-flight recording.
pub fn resolve(settings: &AppSettings, application: Option<&str>) -> AppSettings {
    let mut resolved = settings.clone();
    let Some(application) = application else {
        return resolved;
    };
    let key = application_key(application);
    let Some(profile) = settings
        .application_profiles
        .iter()
        .find(|profile| application_key(&profile.application) == key)
    else {
        return resolved;
    };
    if let Some(value) = profile.cleanup_enabled {
        resolved.cleanup_enabled = value;
    }
    if let Some(value) = profile.auto_polish {
        resolved.auto_polish = value;
    }
    if let Some(value) = profile.polish_style {
        resolved.polish_style = value;
    }
    if !profile.instructions.trim().is_empty() {
        if !resolved.custom_instructions.is_empty() {
            resolved.custom_instructions.push('\n');
        }
        resolved
            .custom_instructions
            .push_str(profile.instructions.trim());
    }
    resolved
}
