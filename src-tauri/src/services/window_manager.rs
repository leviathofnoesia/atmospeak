use anyhow::{Result, anyhow};
use tauri::{
    AppHandle, LogicalSize, Manager, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::services::{app_state::AppState, overlay_window};

pub const SETUP_VIEW: &str = "index.html?view=setup";
pub const HUB_VIEW: &str = "index.html?view=hub";
pub const OVERLAY_VIEW: &str = "index.html?view=overlay";

pub fn setup_is_complete(app: &AppHandle, onboarding_version: &str) -> bool {
    app.state::<AppState>()
        .database
        .lock()
        .load_settings()
        .map(|settings| {
            settings.onboarding_complete
                && settings.onboarding_version == onboarding_version
                && settings
                    .audio_calibration
                    .as_ref()
                    .is_some_and(|calibration| calibration.asr_backend == "host")
        })
        .unwrap_or(false)
}

pub fn ensure_main(app: &AppHandle, setup: bool) -> Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(window);
    }

    let view = if setup { SETUP_VIEW } else { HUB_VIEW };
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(view.into()))
        .title("Atmospeak")
        .inner_size(1000.0, 660.0)
        .min_inner_size(900.0, 620.0)
        .resizable(true)
        .center();
    #[cfg(target_os = "windows")]
    if let Some(arguments) = webview_debug_arguments() {
        builder = builder.additional_browser_args(&arguments);
    }
    let window = builder.build()?;
    let _ = window.set_focus();
    Ok(window)
}

pub fn ensure_overlay(app: &AppHandle) -> Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window("overlay") {
        return Ok(window);
    }

    let mut builder =
        WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App(OVERLAY_VIEW.into()))
            .title("Atmospeak Overlay")
            .inner_size(520.0, 150.0)
            .min_inner_size(420.0, 132.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false);
    #[cfg(target_os = "windows")]
    if let Some(arguments) = webview_debug_arguments() {
        builder = builder.additional_browser_args(&arguments);
    }
    let window = builder.build()?;
    let _ = window.set_size(Size::Logical(LogicalSize::new(520.0, 150.0)));
    Ok(window)
}

#[cfg(target_os = "windows")]
fn webview_debug_arguments() -> Option<String> {
    let port = std::env::var("ATMOSPEAK_WEBVIEW_DEBUG_PORT")
        .ok()?
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)?;
    Some(format!("--remote-debugging-port={port}"))
}

pub fn show_overlay(app: &AppHandle, onboarding_version: &str) -> Result<()> {
    if !setup_is_complete(app, onboarding_version) {
        ensure_main(app, true)?;
        return Err(anyhow!(
            "Finish microphone setup before using the floating control."
        ));
    }
    ensure_overlay(app)?;
    overlay_window::show(app)
}

pub fn finish_setup(app: &AppHandle) -> Result<()> {
    let main = ensure_main(app, false)?;
    main.eval("window.location.replace('/?view=hub')")?;
    ensure_overlay(app)?;
    overlay_window::show(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::models::{AppSettings, AudioCalibrationRecord};
    use chrono::Utc;

    fn complete(settings: &AppSettings, version: &str) -> bool {
        settings.onboarding_complete
            && settings.onboarding_version == version
            && settings
                .audio_calibration
                .as_ref()
                .is_some_and(|calibration| calibration.asr_backend == "host")
    }

    #[test]
    fn legacy_completion_without_calibration_is_rejected() {
        let settings = AppSettings {
            onboarding_complete: true,
            onboarding_version: "phase-a-honest-mvp-v1".to_string(),
            ..AppSettings::default()
        };
        assert!(!complete(&settings, "atmospeak-setup-v2"));
    }

    #[test]
    fn v2_completion_requires_host_calibration() {
        let mut settings = AppSettings {
            onboarding_complete: true,
            onboarding_version: "atmospeak-setup-v2".to_string(),
            ..AppSettings::default()
        };
        assert!(!complete(&settings, "atmospeak-setup-v2"));
        settings.audio_calibration = Some(AudioCalibrationRecord {
            device_name: "Test microphone".to_string(),
            checked_at: Utc::now(),
            rms_dbfs: -30.0,
            peak_dbfs: -12.0,
            snr_db: 20.0,
            model_id: "base.en".to_string(),
            asr_backend: "host".to_string(),
        });
        assert!(complete(&settings, "atmospeak-setup-v2"));
    }
}
