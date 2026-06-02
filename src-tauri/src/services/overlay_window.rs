use anyhow::{Result, anyhow};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewWindow,
};

const OVERLAY_WIDTH: f64 = 420.0;
const OVERLAY_HEIGHT: f64 = 128.0;
const OVERLAY_BOTTOM_MARGIN: f64 = 56.0;

pub fn show_and_reset(app: &AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("overlay")
        .ok_or_else(|| anyhow!("Floating control window is not available."))?;

    let _ = window.unminimize();
    let _ = window.set_size(Size::Logical(LogicalSize::new(
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
    )));
    position_near_bottom_center(&window);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = app.emit(
        "wind-speak://overlay-visibility",
        "Floating control shown and reset above other windows.",
    );
    Ok(())
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
