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
            let (x, y) = clamp_to_visible(&window, x, y);
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
    let _ = app.emit("atmospeak://overlay-position-resetting", ());
    let _ = app.emit("wind-speak://overlay-position-resetting", ());
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
    strip_system_chrome(window);
}

/// Windows 11 draws rounded corners and a 1px border on *every* top-level window,
/// including undecorated ones — which frames the transparent companion in a visible
/// rectangle. `shadow: false` in tauri.conf.json removes the drop shadow; these two
/// DWM attributes remove the corner rounding and the border itself.
#[cfg(target_os = "windows")]
fn strip_system_chrome(window: &WebviewWindow) {
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
            DwmSetWindowAttribute,
        },
    };

    /// `DWMWA_COLOR_NONE` — suppress the border entirely.
    const COLOR_NONE: u32 = 0xFFFF_FFFE;

    let Ok(handle) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(handle.0 as _);

    unsafe {
        let corners = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corners as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&corners) as u32,
        );
        let border = COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn strip_system_chrome(_window: &WebviewWindow) {}

/// Remember where the companion was dropped. Kept in its own file rather than
/// `AppSettings`, which is contract-locked to 12 fields.
pub fn save_position(app: &AppHandle, x: i32, y: i32) {
    let Some(path) = position_path(app) else {
        return;
    };
    let body = serde_json::json!({ "x": x, "y": y });
    let _ = std::fs::write(path, body.to_string());
}

/// Keep enough of the dock on a monitor to grab again. Without this, a drag that
/// ends past the desktop edge (or a monitor that later disappears) strands the
/// companion somewhere you cannot reach it.
fn clamp_to_visible(window: &WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    /// How much of the dock must stay reachable.
    const KEEP_VISIBLE: i32 = 80;

    let Ok(monitors) = window.available_monitors() else {
        return (x, y);
    };
    if monitors.is_empty() {
        return (x, y);
    }

    let size = window
        .outer_size()
        .map(|s| (s.width as i32, s.height as i32))
        .unwrap_or((520, 150));

    // Anchor on whichever monitor holds the dock's centre; fall back to the first.
    let centre = (x + size.0 / 2, y + size.1 / 2);
    let monitor = monitors
        .iter()
        .find(|monitor| {
            let pos = monitor.position();
            let dim = monitor.size();
            centre.0 >= pos.x
                && centre.0 < pos.x + dim.width as i32
                && centre.1 >= pos.y
                && centre.1 < pos.y + dim.height as i32
        })
        .unwrap_or(&monitors[0]);

    let pos = monitor.position();
    let dim = monitor.size();
    // Allow it to hang off an edge, but never by more than it can be grabbed back.
    let min_x = pos.x - (size.0 - KEEP_VISIBLE);
    let min_y = pos.y;
    let max_x = pos.x + dim.width as i32 - KEEP_VISIBLE;
    let max_y = pos.y + dim.height as i32 - KEEP_VISIBLE;
    (
        x.clamp(min_x, max_x.max(min_x)),
        y.clamp(min_y, max_y.max(min_y)),
    )
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
