use anyhow::{Result, anyhow};
use serde::Deserialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewWindow,
};

use crate::services::app_state::AppState;

const OVERLAY_WIDTH: f64 = 520.0;
const OVERLAY_HEIGHT: f64 = 150.0;
const OVERLAY_BOTTOM_MARGIN: f64 = 56.0;
const POSITION_FILE: &str = "overlay-position.json";

/// A visible client-area rectangle that should receive pointer input.
///
/// The overlay webview remains transparent outside these rectangles. The
/// frontend sends fresh rectangles whenever the dock changes state, so hidden
/// controls never leave an invisible hit target behind.
#[derive(Debug, Clone, Deserialize)]
pub struct OverlayHitRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

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

/// Restrict the native overlay window to the currently visible dock controls.
///
/// Windows treats the area outside a window region as belonging to the window
/// behind it. This keeps the floating companion from intercepting clicks in its
/// transparent margins while preserving normal WebView event handling inside
/// the orb and its visible buttons. Logical client coordinates are converted to
/// physical pixels using the window's current scale factor.
#[cfg(target_os = "windows")]
pub fn set_hit_regions(app: &AppHandle, regions: Vec<OverlayHitRegion>) -> Result<()> {
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn},
    };

    let window = overlay(app)?;
    let handle = window
        .hwnd()
        .map_err(|error| anyhow!("could not access overlay window: {error}"))?;
    let hwnd = HWND(handle.0 as _);
    let scale = window
        .scale_factor()
        .map_err(|error| anyhow!("could not read overlay scale factor: {error}"))?;
    let mut combined = None;
    for region in regions {
        if !region.x.is_finite()
            || !region.y.is_finite()
            || !region.width.is_finite()
            || !region.height.is_finite()
            || region.width <= 0.0
            || region.height <= 0.0
        {
            continue;
        }
        let left = (region.x * scale).floor();
        let top = (region.y * scale).floor();
        let right = ((region.x + region.width) * scale).ceil();
        let bottom = ((region.y + region.height) * scale).ceil();
        if !left.is_finite()
            || !top.is_finite()
            || !right.is_finite()
            || !bottom.is_finite()
            || right <= left
            || bottom <= top
            || left < i32::MIN as f64
            || top < i32::MIN as f64
            || right > i32::MAX as f64
            || bottom > i32::MAX as f64
        {
            continue;
        }
        let rect = unsafe {
            CreateRectRgn(left as i32, top as i32, right as i32, bottom as i32)
        };
        if rect.0.is_null() {
            continue;
        }
        if let Some(destination) = combined {
            unsafe {
                CombineRgn(Some(destination), Some(destination), Some(rect), RGN_OR);
                let _ = DeleteObject(rect.into());
            }
        } else {
            combined = Some(rect);
        }
    }

    // Keep the whole transparent window click-through while the DOM is between
    // states or before its first measurement. Passing None would remove the
    // region and restore the default full-window hit area, so use a zero-sized
    // region when no visible controls were supplied.
    let combined = combined.or_else(|| unsafe { Some(CreateRectRgn(0, 0, 0, 0)) });
    let result = unsafe { SetWindowRgn(hwnd, combined, true) };
    if result == 0 {
        if let Some(region) = combined {
            unsafe {
                let _ = DeleteObject(region.into());
            }
        }
        return Err(anyhow!("Windows rejected the overlay hit region"));
    }
    // SetWindowRgn takes ownership of a successful region handle.
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_hit_regions(_app: &AppHandle, _regions: Vec<OverlayHitRegion>) -> Result<()> {
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

/// How much of the dock must stay reachable after a drag.
const KEEP_VISIBLE: i32 = 80;

/// Axis-aligned monitor rectangle used by the pure clamp helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Keep enough of the dock on a monitor to grab again. Pure so it can be unit-
/// tested without a live window: a drag that ends past the desktop edge (or a
/// monitor that later disappears) must not strand the companion.
pub fn clamp_position(
    x: i32,
    y: i32,
    window_width: i32,
    window_height: i32,
    monitors: &[MonitorRect],
) -> (i32, i32) {
    if monitors.is_empty() {
        return (x, y);
    }

    // Anchor on whichever monitor holds the dock's centre; fall back to the first.
    let centre = (x + window_width / 2, y + window_height / 2);
    let monitor = monitors
        .iter()
        .find(|monitor| {
            centre.0 >= monitor.x
                && centre.0 < monitor.x + monitor.width
                && centre.1 >= monitor.y
                && centre.1 < monitor.y + monitor.height
        })
        .unwrap_or(&monitors[0]);

    // Allow it to hang off an edge, but never by more than it can be grabbed back.
    let min_x = monitor.x - (window_width - KEEP_VISIBLE);
    let min_y = monitor.y;
    let max_x = monitor.x + monitor.width - KEEP_VISIBLE;
    let max_y = monitor.y + monitor.height - KEEP_VISIBLE;
    (
        x.clamp(min_x, max_x.max(min_x)),
        y.clamp(min_y, max_y.max(min_y)),
    )
}

/// Remember where the companion was dropped, clamped so the persisted spot is
/// always reachable. Returns the clamped coordinates so the frontend can settle
/// the live window when the OS drag ended off-screen. Kept in its own file
/// rather than `AppSettings`, which is contract-locked to 12 fields.
pub fn save_position(app: &AppHandle, x: i32, y: i32) -> (i32, i32) {
    let (x, y) = match overlay(app).ok() {
        Some(window) => clamp_to_visible(&window, x, y),
        None => (x, y),
    };
    if let Some(path) = position_path(app) {
        let body = serde_json::json!({ "x": x, "y": y });
        let _ = std::fs::write(path, body.to_string());
    }
    (x, y)
}

/// Keep enough of the dock on a monitor to grab again. Without this, a drag that
/// ends past the desktop edge (or a monitor that later disappears) strands the
/// companion somewhere you cannot reach it.
fn clamp_to_visible(window: &WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    let Ok(monitors) = window.available_monitors() else {
        return (x, y);
    };
    let size = window
        .outer_size()
        .map(|s| (s.width as i32, s.height as i32))
        .unwrap_or((520, 150));
    let rects = monitors
        .iter()
        .map(|monitor| {
            let pos = monitor.position();
            let dim = monitor.size();
            MonitorRect {
                x: pos.x,
                y: pos.y,
                width: dim.width as i32,
                height: dim.height as i32,
            }
        })
        .collect::<Vec<_>>();
    clamp_position(x, y, size.0, size.1, &rects)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn primary() -> MonitorRect {
        MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn clamp_leaves_on_screen_positions_alone() {
        assert_eq!(clamp_position(100, 200, 520, 150, &[primary()]), (100, 200));
    }

    #[test]
    fn clamp_pulls_fully_off_screen_positions_back() {
        // Flung far left: leave KEEP_VISIBLE (80) of the dock on-screen.
        assert_eq!(
            clamp_position(-10_000, 400, 520, 150, &[primary()]),
            (80 - 520, 400)
        );
        // Flung above the top: y cannot go above the monitor origin.
        assert_eq!(clamp_position(100, -500, 520, 150, &[primary()]), (100, 0));
        // Flung past the right/bottom edges.
        assert_eq!(
            clamp_position(5_000, 5_000, 520, 150, &[primary()]),
            (1920 - 80, 1080 - 80)
        );
    }

    #[test]
    fn clamp_anchors_on_the_monitor_holding_the_centre() {
        let left = MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let right = MonitorRect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        // Centre on the right monitor, y slightly above it → clamp y against that
        // monitor (KEEP_VISIBLE height still leaves the centre inside).
        let (x, y) = clamp_position(2_500, -40, 520, 150, &[left, right]);
        assert_eq!((x, y), (2_500, 0));
        // Flung far left of both monitors → fall back to the first monitor.
        let (x, y) = clamp_position(-5_000, 100, 520, 150, &[left, right]);
        assert_eq!((x, y), (80 - 520, 100));
    }

    #[test]
    fn clamp_is_a_no_op_without_monitors() {
        assert_eq!(clamp_position(-999, -999, 520, 150, &[]), (-999, -999));
    }
}
