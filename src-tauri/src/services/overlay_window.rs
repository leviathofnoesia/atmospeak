use anyhow::{Result, anyhow};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewWindow,
};

use crate::services::app_state::AppState;

const OVERLAY_WIDTH: f64 = 520.0;
const OVERLAY_HEIGHT: f64 = 150.0;
const OVERLAY_BOTTOM_MARGIN: f64 = 56.0;
const POSITION_FILE: &str = "overlay-position.json";

/// Show the overlay where the user last dropped it, falling back to bottom-centre.
/// Used at startup so a moved companion stays put across restarts.
pub fn show(app: &AppHandle) -> Result<()> {
    let window = overlay(app)?;
    prepare(&window);
    match load_position(app) {
        Some((x, y)) => {
            let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
        }
        None => position_near_bottom_center(&window),
    }
    let _ = window.show();
    Ok(())
}

/// Show the overlay and return it to its default spot, for when the user has
/// parked it somewhere unreachable (off-screen, behind a since-removed monitor).
pub fn show_and_reset(app: &AppHandle) -> Result<()> {
    let window = overlay(app)?;
    prepare(&window);
    position_near_bottom_center(&window);
    clear_position(app);
    let _ = window.show();
    let _ = app.emit(
        "wind-speak://overlay-visibility",
        "Floating control shown and reset above other windows.",
    );
    Ok(())
}

fn overlay(app: &AppHandle) -> Result<WebviewWindow> {
    app.get_webview_window("overlay")
        .ok_or_else(|| anyhow!("Floating control window is not available."))
}

fn prepare(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.set_size(Size::Logical(LogicalSize::new(
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
    )));
    let _ = window.set_always_on_top(true);
}

/// Remember where the companion was dropped. Kept in its own file rather than
/// `AppSettings`, which is contract-locked to 12 fields.
pub fn save_position(app: &AppHandle, x: i32, y: i32) {
    let Some(path) = position_path(app) else {
        return;
    };
    let body = serde_json::json!({ "x": x, "y": y });
    let _ = std::fs::write(path, body.to_string());
}

fn load_position(app: &AppHandle) -> Option<(i32, i32)> {
    let raw = std::fs::read_to_string(position_path(app)?).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let x = parsed.get("x")?.as_i64()? as i32;
    let y = parsed.get("y")?.as_i64()? as i32;
    Some((x, y))
}

fn clear_position(app: &AppHandle) {
    if let Some(path) = position_path(app) {
        let _ = std::fs::remove_file(path);
    }
}

fn position_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.try_state::<AppState>()?.app_dir.join(POSITION_FILE))
}

fn position_near_bottom_center(window: &WebviewWindow) {
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => monitor,
        _ => {
            let _ = window.center();
            return;
        }
    };

    let scale = monitor.scale_factor();
    let monitor_size = monitor.size();
    let monitor_position = monitor.position();
    let width = OVERLAY_WIDTH * scale;
    let height = OVERLAY_HEIGHT * scale;
    let margin = OVERLAY_BOTTOM_MARGIN * scale;

    let x = monitor_position.x + ((monitor_size.width as f64 - width) / 2.0).max(0.0) as i32;
    let y = monitor_position.y
        + (monitor_size.height as f64 - height - margin)
            .max(0.0)
            .round() as i32;

    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}
