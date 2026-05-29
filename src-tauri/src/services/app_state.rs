use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::sync::Arc;

use crate::{db::Database, models::ShortcutStatus, services::recorder::RecorderService};

pub struct AppState {
    pub database: Mutex<Database>,
    pub recorder: RecorderService,
    pub shortcut_status: Arc<Mutex<ShortcutStatus>>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let app_dir = dirs::data_local_dir()
            .context("failed to resolve local application data directory")?
            .join("Wind Speak");
        let recordings_dir = app_dir.join("recordings");
        std::fs::create_dir_all(&recordings_dir)
            .context("failed to create recordings directory")?;

        Ok(Self {
            database: Mutex::new(Database::open(app_dir.clone())?),
            recorder: RecorderService::new(recordings_dir),
            shortcut_status: Arc::new(Mutex::new(ShortcutStatus::default())),
        })
    }
}
