use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use crate::{
    db::Database,
    models::{RuntimeEvent, ShortcutStatus},
    services::recorder::RecorderService,
};

/// The application's data plane: every long-lived piece of state lives here.
///
/// Most fields are `pub` so existing consumers can read/write them through the
/// `parking_lot::Mutex` directly. New code should prefer the named accessor
/// methods (`[set|with]_shortcuts_paused`, `[set|with]_shortcut_test_active`,
/// `record_event`, `last_target_window`, `cancel_retention_sweeper`) so that
/// callers don't need to know the internal layout. The aim of the deepening
/// is to make AppState the single place behaviour is altered, not to
/// immediately hide every field.
pub struct AppState {
    /// Resolved on-disk app data directory; used by diagnostics, streaming,
    /// and the retention sweeper for absolute paths.
    pub app_dir: PathBuf,
    /// SQLite-backed persistence for settings, dictionary, snippets, sessions.
    pub database: Mutex<Database>,
    /// CPAL recorder and FFT analyser for the active recording.
    pub recorder: RecorderService,
    /// Current state of the global shortcut (registered, paused, hotkey label).
    pub shortcut_status: Arc<Mutex<ShortcutStatus>>,
    /// Whether shortcut handling is paused (e.g. while another app is focused).
    pub shortcuts_paused: Arc<Mutex<bool>>,
    /// Set to true while the user is running the "press your shortcut" test.
    pub shortcut_test_active: Arc<Mutex<bool>>,
    /// Last external (non-Wind Speak) target window we pasted into. Lets us
    /// restore focus to the same window across the next injection. Stored as
    /// a raw `isize` window handle until `injection` grows a typed
    /// `InjectionTarget` value.
    pub last_external_target_window: Arc<Mutex<Option<isize>>>,
    /// Bounded log of recent runtime events; surfaced to the UI on demand.
    pub runtime_events: Arc<Mutex<Vec<RuntimeEvent>>>,
    /// Signal the background retention sweeper to exit cleanly on app shutdown.
    pub retention_sweeper_cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let app_dir = resolve_app_dir()?;
        let recordings_dir = app_dir.join("recordings");
        std::fs::create_dir_all(&recordings_dir)
            .context("failed to create recordings directory")?;

        Ok(Self {
            app_dir: app_dir.clone(),
            database: Mutex::new(Database::open(app_dir.clone())?),
            recorder: RecorderService::new(recordings_dir),
            shortcut_status: Arc::new(Mutex::new(ShortcutStatus::default())),
            shortcuts_paused: Arc::new(Mutex::new(false)),
            shortcut_test_active: Arc::new(Mutex::new(false)),
            last_external_target_window: Arc::new(Mutex::new(None)),
            runtime_events: Arc::new(Mutex::new(Vec::new())),
            retention_sweeper_cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    // ---- Named accessors (the seam) ----

    /// Read whether shortcuts are currently paused.
    pub fn shortcuts_paused(&self) -> bool {
        *self.shortcuts_paused.lock()
    }

    /// Atomically set the paused flag and return its new value.
    pub fn set_shortcuts_paused(&self, paused: bool) -> bool {
        *self.shortcuts_paused.lock() = paused;
        paused
    }

    /// Read whether the shortcut test mode is active.
    pub fn shortcut_test_active(&self) -> bool {
        *self.shortcut_test_active.lock()
    }

    /// Set the shortcut test flag.
    pub fn set_shortcut_test_active(&self, active: bool) {
        *self.shortcut_test_active.lock() = active;
    }

    /// Read the most recently captured external target window.
    pub fn last_target_window(&self) -> Option<isize> {
        *self.last_external_target_window.lock()
    }

    /// Replace the external target window.
    pub fn set_last_target_window(&self, target: Option<isize>) {
        *self.last_external_target_window.lock() = target;
    }

    /// Append a runtime event to the bounded log (capped at 200 entries).
    pub fn record_event(&self, event: RuntimeEvent) {
        let mut log = self.runtime_events.lock();
        log.insert(0, event);
        if log.len() > 200 {
            log.truncate(200);
        }
    }

    /// Read the most recent N runtime events (newest first).
    pub fn recent_events(&self, limit: usize) -> Vec<RuntimeEvent> {
        self.runtime_events
            .lock()
            .iter()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Signal the retention sweeper to exit at the next check.
    pub fn cancel_retention_sweeper(&self) {
        self.retention_sweeper_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn resolve_app_dir() -> Result<PathBuf> {
    if let Ok(override_dir) = std::env::var("WIND_SPEAK_APP_DATA_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    Ok(dirs::data_local_dir()
        .context("failed to resolve local application data directory")?
        .join("Wind Speak"))
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::models::RuntimeEvent;
    use chrono::Utc;
    use std::sync::atomic::Ordering;

    /// AppState can be built with all named fields populated and the named
    /// accessors return the expected values. Exercises every new seam
    /// without needing the Tauri runtime.
    #[test]
    fn named_accessors_round_trip_state() {
        let key = "WIND_SPEAK_APP_DATA_DIR";
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, r"C:\temp\wind-speak-appstate-test");
        }

        let state = AppState::new().expect("app state");

        // Defaults
        assert!(!state.shortcuts_paused());
        assert!(!state.shortcut_test_active());
        assert!(state.last_target_window().is_none());
        assert!(state.recent_events(10).is_empty());
        assert!(!state.retention_sweeper_cancel.load(Ordering::Relaxed));

        // Round trip shortcuts_paused
        let new_value = state.set_shortcuts_paused(true);
        assert!(new_value);
        assert!(state.shortcuts_paused());

        // Round trip shortcut_test_active
        state.set_shortcut_test_active(true);
        assert!(state.shortcut_test_active());
        state.set_shortcut_test_active(false);
        assert!(!state.shortcut_test_active());

        // Round trip record_event / recent_events
        for i in 0..5 {
            state.record_event(RuntimeEvent {
                created_at: Utc::now(),
                kind: "test".to_string(),
                message: format!("event-{i}"),
            });
        }
        let events = state.recent_events(3);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].message, "event-4");
        assert_eq!(events[2].message, "event-2");

        // Round trip retention cancel
        state.cancel_retention_sweeper();
        assert!(state.retention_sweeper_cancel.load(Ordering::Relaxed));

        match previous {
            Some(value) => unsafe {
                std::env::set_var(key, value);
            },
            None => unsafe {
                std::env::remove_var(key);
            },
        }
    }
}
