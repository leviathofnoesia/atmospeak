use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::{
    io::Seek,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{AppHandle, Manager};

use crate::{
    db::Database,
    models::{RuntimeEvent, ShortcutStatus, StageMetrics, TranscriptSession},
    services::{
        asr_host::AsrHost, dictation_engine::EngineHandle, live_paste::LivePasteBuffer,
        llama_host::LlamaHost, recorder::RecorderService, streaming_asr::StreamingAsr,
    },
};

pub struct AppState {
    pub app_dir: PathBuf,
    pub database: Mutex<Database>,
    pub dictation_snapshot: Mutex<crate::services::dictation_state::DictationState>,
    pub history_request_generation: AtomicU64,
    pub history_request: Mutex<Option<crate::services::history_actions::HistoryRequest>>,
    pub recorder: RecorderService,
    pub shortcut_status: Arc<Mutex<ShortcutStatus>>,
    pub shortcuts_paused: Arc<Mutex<bool>>,
    pub shortcut_test_active: Arc<Mutex<bool>>,
    shortcut_test_deadline_ms: AtomicU64,
    pub shortcut_capture_active: Arc<Mutex<bool>>,
    pub last_external_target_window: Arc<Mutex<Option<isize>>>,
    pub runtime_events: Arc<Mutex<Vec<RuntimeEvent>>>,
    pub model_download_cancel: Arc<AtomicBool>,
    level_stream_generation: AtomicU64,
    model_download_active: Mutex<Option<String>>,
    engine: Mutex<Option<EngineHandle>>,
    last_metrics: Mutex<Option<StageMetrics>>,
    asr_host: Mutex<Option<Arc<AsrHost>>>,
    llama_host: Mutex<Option<Arc<LlamaHost>>>,
    streaming_asr: Mutex<Option<Arc<StreamingAsr>>>,
    streaming_asr_generation: AtomicU64,
    pub live_paste: Arc<LivePasteBuffer>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let (app_dir, from_env_override) = resolve_app_dir()?;
        std::fs::create_dir_all(&app_dir).context("failed to create application data directory")?;
        maybe_migrate_from_legacy(&app_dir, from_env_override);

        let recordings_dir = app_dir.join("recordings");
        std::fs::create_dir_all(&recordings_dir)
            .context("failed to create recordings directory")?;
        let database = Database::open(app_dir.clone())?;
        let recorder = RecorderService::new(recordings_dir.clone());
        recover_interrupted_recordings(&database, &recorder);
        let runtime_events = database.list_runtime_events(200).unwrap_or_default();
        let retention_days = database
            .load_settings()
            .map(|settings| settings.transcript_retention_days)
            .unwrap_or(0);
        for raw_path in database.prune_sessions(retention_days)? {
            let path = PathBuf::from(raw_path);
            if path.starts_with(&recordings_dir) {
                let _ = std::fs::remove_file(path);
            }
        }

        Ok(Self {
            app_dir: app_dir.clone(),
            database: Mutex::new(database),
            history_request: Mutex::new(None),
            history_request_generation: AtomicU64::new(0),
            dictation_snapshot: Mutex::new(Default::default()),
            recorder,
            shortcut_status: Arc::new(Mutex::new(ShortcutStatus::default())),
            shortcuts_paused: Arc::new(Mutex::new(false)),
            shortcut_test_active: Arc::new(Mutex::new(false)),
            shortcut_test_deadline_ms: AtomicU64::new(0),
            shortcut_capture_active: Arc::new(Mutex::new(false)),
            last_external_target_window: Arc::new(Mutex::new(None)),
            runtime_events: Arc::new(Mutex::new(runtime_events)),
            model_download_cancel: Arc::new(AtomicBool::new(false)),
            level_stream_generation: AtomicU64::new(0),
            model_download_active: Mutex::new(None),
            engine: Mutex::new(None),
            last_metrics: Mutex::new(None),
            asr_host: Mutex::new(None),
            llama_host: Mutex::new(None),
            streaming_asr: Mutex::new(None),
            streaming_asr_generation: AtomicU64::new(0),
            live_paste: Arc::new(LivePasteBuffer::new()),
        })
    }

    pub fn set_asr_host(&self, host: Arc<AsrHost>) {
        *self.asr_host.lock() = Some(host);
    }

    pub fn asr_host(&self) -> Option<Arc<AsrHost>> {
        self.asr_host.lock().clone()
    }

    /// Convenience for call sites that only hold an `AppHandle`.
    pub fn asr_host_from(app: &AppHandle) -> Option<Arc<AsrHost>> {
        app.try_state::<AppState>()?.asr_host()
    }

    pub fn shutdown_asr_host(&self) {
        if let Some(host) = self.asr_host.lock().take() {
            host.shutdown();
        }
    }

    pub fn set_llama_host(&self, host: Arc<LlamaHost>) {
        if let Some(previous) = self.llama_host.lock().replace(host) {
            previous.shutdown();
        }
    }

    pub fn llama_host(&self) -> Option<Arc<LlamaHost>> {
        self.llama_host.lock().clone()
    }

    pub fn llama_host_from(app: &AppHandle) -> Option<Arc<LlamaHost>> {
        app.try_state::<AppState>()?.llama_host()
    }

    pub fn shutdown_llama_host(&self) {
        if let Some(host) = self.llama_host.lock().take() {
            host.shutdown();
        }
    }

    pub fn set_engine(&self, handle: EngineHandle) {
        *self.engine.lock() = Some(handle);
    }

    pub fn engine(&self) -> Option<EngineHandle> {
        self.engine.lock().clone()
    }

    pub fn set_last_metrics(&self, metrics: StageMetrics) {
        *self.last_metrics.lock() = Some(metrics);
    }

    pub fn last_metrics(&self) -> Option<StageMetrics> {
        self.last_metrics.lock().clone()
    }

    pub fn shortcuts_paused(&self) -> bool {
        *self.shortcuts_paused.lock()
    }

    pub fn set_shortcuts_paused(&self, paused: bool) -> bool {
        *self.shortcuts_paused.lock() = paused;
        paused
    }

    pub fn shortcut_test_active(&self) -> bool {
        let mut active = self.shortcut_test_active.lock();
        if !*active {
            return false;
        }
        let deadline = self.shortcut_test_deadline_ms.load(Ordering::Relaxed);
        if deadline > 0 && now_ms() > deadline {
            *active = false;
            self.shortcut_test_deadline_ms.store(0, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Invalidates every in-flight warmup and returns the generation owned by
    /// the caller about to create a new host.
    pub fn begin_streaming_asr_warmup(&self) -> u64 {
        self.streaming_asr_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn streaming_asr_is_current(&self, generation: u64) -> bool {
        self.streaming_asr_generation.load(Ordering::Acquire) == generation
    }

    pub fn set_streaming_asr_if_current(&self, generation: u64, host: Arc<StreamingAsr>) -> bool {
        // Coordinate the generation test and publication with shutdown's slot
        // lock. A shutdown that wins either invalidates us before this point or
        // removes this just-published host immediately afterwards.
        let mut candidate = Some(host);
        let previous = {
            let mut slot = self.streaming_asr.lock();
            if !self.streaming_asr_is_current(generation) {
                None
            } else {
                Some(slot.replace(candidate.take().expect("candidate host is available")))
            }
        };
        let Some(previous) = previous else {
            candidate.expect("stale warmup retains its host").shutdown();
            return false;
        };
        if let Some(previous) = previous {
            previous.shutdown();
        }
        true
    }

    pub fn streaming_asr(&self) -> Option<Arc<StreamingAsr>> {
        self.streaming_asr.lock().clone()
    }

    pub fn shutdown_streaming_asr(&self) {
        self.streaming_asr_generation.fetch_add(1, Ordering::AcqRel);
        if let Some(host) = self.streaming_asr.lock().take() {
            host.shutdown();
        }
    }

    pub fn set_shortcut_test_active(&self, active: bool) {
        *self.shortcut_test_active.lock() = active;
        self.shortcut_test_deadline_ms.store(
            active.then(|| now_ms() + 20_000).unwrap_or(0),
            Ordering::Relaxed,
        );
    }

    pub fn shortcut_capture_active(&self) -> bool {
        *self.shortcut_capture_active.lock()
    }

    pub fn set_shortcut_capture_active(&self, active: bool) {
        *self.shortcut_capture_active.lock() = active;
    }

    pub fn clear_shortcut_interaction_state(&self) {
        *self.shortcut_test_active.lock() = false;
        self.shortcut_test_deadline_ms.store(0, Ordering::Relaxed);
        *self.shortcut_capture_active.lock() = false;
    }

    pub fn last_target_window(&self) -> Option<isize> {
        *self.last_external_target_window.lock()
    }

    pub fn set_last_target_window(&self, target: Option<isize>) {
        *self.last_external_target_window.lock() = target;
    }

    pub fn record_event(&self, event: RuntimeEvent) {
        self.record_events(vec![event]);
    }

    pub fn record_events(&self, events: Vec<RuntimeEvent>) {
        if let Err(error) = self.database.lock().insert_runtime_events(&events) {
            eprintln!("failed to persist runtime events: {error}");
        }
        let mut log = self.runtime_events.lock();
        for event in events {
            log.insert(0, event);
        }
        log.truncate(200);
    }

    pub fn recent_events(&self, limit: usize) -> Vec<RuntimeEvent> {
        self.runtime_events
            .lock()
            .iter()
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn begin_model_download(&self, model_id: &str) -> Result<()> {
        let mut active = self.model_download_active.lock();
        if let Some(current) = active.as_ref() {
            anyhow::bail!("model download already in progress: {current}");
        }
        self.model_download_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        *active = Some(model_id.to_string());
        Ok(())
    }

    pub fn finish_model_download(&self) {
        *self.model_download_active.lock() = None;
        self.model_download_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn cancel_model_download(&self) -> bool {
        let active = self.model_download_active.lock().is_some();
        if active {
            self.model_download_cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        active
    }

    pub fn begin_level_stream(&self) -> u64 {
        self.level_stream_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn end_level_stream(&self) {
        self.level_stream_generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn level_stream_is_current(&self, generation: u64) -> bool {
        self.level_stream_generation.load(Ordering::Relaxed) == generation
    }
}

/// Reconcile capture checkpoints before the engine starts accepting new work.
/// The marker is acknowledged only after the corresponding History row is committed.
/// A valid source already referenced by History is preserved and the marker is simply
/// retired, so recovery never replaces a known-good WAV with a partial prefix.
fn recover_interrupted_recordings(database: &Database, recorder: &RecorderService) {
    let recoverable = match recorder.list_recoverable_recordings() {
        Ok(recordings) => recordings,
        Err(error) => {
            eprintln!("atmospeak recovery: failed to inspect capture checkpoints: {error}");
            return;
        }
    };

    for recording in recoverable {
        let existing = match database.get_session(&recording.id) {
            Ok(session) => session,
            Err(error) => {
                eprintln!(
                    "atmospeak recovery: failed to inspect History for {}: {error}",
                    recording.id
                );
                continue;
            }
        };

        if existing
            .as_ref()
            .is_some_and(|session| audio_source_is_usable(&session.audio_path))
        {
            if let Err(error) = recorder.acknowledge_recovered_recording(&recording.id) {
                eprintln!(
                    "atmospeak recovery: failed to acknowledge already-saved {}: {error}",
                    recording.id
                );
            }
            continue;
        }

        let finished = match recorder.recover_recording(&recording.id) {
            Ok(finished) => finished,
            Err(error) => {
                eprintln!(
                    "atmospeak recovery: failed to materialize {}: {error}",
                    recording.id
                );
                continue;
            }
        };

        let session = match existing {
            Some(mut session) => {
                session.audio_path = finished.path.to_string_lossy().to_string();
                session.duration_ms = finished.duration_ms;
                session
            }
            None => TranscriptSession {
                id: finished.id.clone(),
                raw_text: String::new(),
                cleaned_text: String::new(),
                polished_text: None,
                prefer_polished: true,
                audio_path: finished.path.to_string_lossy().to_string(),
                duration_ms: finished.duration_ms,
                word_count: 0,
                injected: false,
                source_application: None,
                created_at: chrono::Utc::now(),
            },
        };
        if let Err(error) = database.insert_session(&session) {
            eprintln!(
                "atmospeak recovery: failed to commit {} to History: {error}",
                session.id
            );
            continue;
        }
        if let Err(error) = recorder.acknowledge_recovered_recording(&recording.id) {
            eprintln!(
                "atmospeak recovery: failed to acknowledge {} after History commit: {error}",
                recording.id
            );
        }
    }
}

fn audio_source_is_usable(path: &str) -> bool {
    let path = PathBuf::from(path);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    let Ok(reader) = hound::WavReader::open(&path) else {
        return false;
    };
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != 16_000
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return false;
    }

    // WavReader has consumed the RIFF/fmt/data headers and is positioned at
    // the PCM payload. Compare its declared frame count with the bytes that
    // physically remain, so a torn data length cannot masquerade as a source.
    let declared_frames = u64::from(reader.duration());
    let mut file = reader.into_inner();
    let Ok(data_start) = file.stream_position() else {
        return false;
    };
    let physical_pcm_bytes = metadata.len().saturating_sub(data_start);
    let expected_pcm_bytes = declared_frames * 2;
    declared_frames > 0 && physical_pcm_bytes == expected_pcm_bytes
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn resolve_app_dir() -> Result<(PathBuf, bool)> {
    if let Ok(override_dir) = std::env::var("ATMOSPEAK_APP_DATA_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return Ok((PathBuf::from(trimmed), true));
        }
    }
    if let Ok(override_dir) = std::env::var("WIND_SPEAK_APP_DATA_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return Ok((PathBuf::from(trimmed), true));
        }
    }

    Ok((
        dirs::data_local_dir()
            .context("failed to resolve local application data directory")?
            .join("Atmospeak"),
        false,
    ))
}

fn maybe_migrate_from_legacy(app_dir: &PathBuf, from_env_override: bool) {
    // SAFETY: never copy production profile into a test/dev override path
    if from_env_override {
        return;
    }

    let Some(local) = dirs::data_local_dir() else {
        return;
    };
    let legacy = local.join("Wind Speak");
    let marker = app_dir.join("migrated-from-wind-speak.json");
    let new_db = app_dir.join("wind-speak.sqlite3");
    let legacy_db = legacy.join("wind-speak.sqlite3");

    if marker.exists() {
        return;
    }
    if new_db.exists() {
        if let Ok(meta) = std::fs::metadata(&new_db) {
            if meta.len() > 0 {
                let _ = write_marker(&marker, &legacy);
                return;
            }
        }
    }
    if !legacy_db.exists() {
        return;
    }

    if let Err(error) = std::fs::create_dir_all(app_dir) {
        eprintln!("atmospeak migrate: create app_dir failed: {error}");
        return;
    }
    if let Err(error) = std::fs::copy(&legacy_db, &new_db) {
        eprintln!("atmospeak migrate: copy db failed: {error}");
        return;
    }
    let legacy_recordings = legacy.join("recordings");
    let new_recordings = app_dir.join("recordings");
    if legacy_recordings.is_dir() {
        if let Err(error) = copy_dir_recursive(&legacy_recordings, &new_recordings) {
            eprintln!("atmospeak migrate: copy recordings failed: {error}");
        }
    }
    let _ = write_marker(&marker, &legacy);
    eprintln!(
        "atmospeak migrate: migrated profile from {} to {}",
        legacy.display(),
        app_dir.display()
    );
}

fn write_marker(marker: &PathBuf, legacy: &PathBuf) -> std::io::Result<()> {
    let body = serde_json::json!({
        "from": legacy.display().to_string(),
        "at": chrono::Utc::now().to_rfc3339(),
        "ok": true,
    });
    std::fs::write(marker, body.to_string())
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppState, audio_source_is_usable};
    use crate::models::RuntimeEvent;
    use chrono::Utc;
    use std::sync::atomic::Ordering;

    #[test]
    fn named_accessors_round_trip_state() {
        let key = "WIND_SPEAK_APP_DATA_DIR";
        let atmospeak_key = "ATMOSPEAK_APP_DATA_DIR";
        let previous = std::env::var(key).ok();
        let previous_atmospeak = std::env::var(atmospeak_key).ok();
        let temp = tempfile::tempdir().expect("app state tempdir");
        unsafe {
            std::env::set_var(key, temp.path());
            std::env::set_var(atmospeak_key, temp.path());
        }

        let state = AppState::new().expect("app state");

        assert!(!state.shortcuts_paused());
        assert!(!state.shortcut_test_active());
        assert!(state.last_target_window().is_none());
        assert!(state.recent_events(10).is_empty());

        let new_value = state.set_shortcuts_paused(true);
        assert!(new_value);
        assert!(state.shortcuts_paused());

        state.set_shortcut_test_active(true);
        assert!(state.shortcut_test_active());
        state.set_shortcut_capture_active(true);
        state.clear_shortcut_interaction_state();
        assert!(!state.shortcut_test_active());
        assert!(!state.shortcut_capture_active());
        state.set_shortcut_test_active(false);
        assert!(!state.shortcut_test_active());

        state.record_event(RuntimeEvent {
            created_at: Utc::now(),
            kind: "test".into(),
            message: "event-0".into(),
        });
        state.record_events(
            (1..5)
                .map(|i| RuntimeEvent {
                    created_at: Utc::now(),
                    kind: "test".into(),
                    message: format!("event-{i}"),
                })
                .collect(),
        );
        assert_eq!(
            state.database.lock().list_runtime_events(10).unwrap().len(),
            5
        );
        let events = state.recent_events(3);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].message, "event-4");
        assert_eq!(events[2].message, "event-2");

        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        match previous_atmospeak {
            Some(value) => unsafe { std::env::set_var(atmospeak_key, value) },
            None => unsafe { std::env::remove_var(atmospeak_key) },
        }
    }
    #[test]
    fn abandoned_shortcut_test_lease_expires() {
        let wind_key = "WIND_SPEAK_APP_DATA_DIR";
        let atmospeak_key = "ATMOSPEAK_APP_DATA_DIR";
        let previous_wind = std::env::var(wind_key).ok();
        let previous_atmospeak = std::env::var(atmospeak_key).ok();
        let temp = tempfile::tempdir().expect("app state tempdir");
        unsafe {
            // Set both names so an outer baseline override cannot redirect this
            // test into a shared profile.
            std::env::set_var(wind_key, temp.path());
            std::env::set_var(atmospeak_key, temp.path());
        }

        let state = AppState::new().expect("app state");
        state.set_shortcut_test_active(true);
        state.shortcut_test_deadline_ms.store(1, Ordering::Relaxed);

        assert!(!state.shortcut_test_active());
        assert!(!*state.shortcut_test_active.lock());

        match previous_wind {
            Some(value) => unsafe { std::env::set_var(wind_key, value) },
            None => unsafe { std::env::remove_var(wind_key) },
        }
        match previous_atmospeak {
            Some(value) => unsafe { std::env::set_var(atmospeak_key, value) },
            None => unsafe { std::env::remove_var(atmospeak_key) },
        }
    }

    #[test]
    fn audio_source_validation_requires_complete_pcm_payload() {
        let temp = tempfile::tempdir().expect("audio validation tempdir");
        let path = temp.path().join("recording.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("wav writer");
        for sample in [0_i16, 1, -1, 2] {
            writer.write_sample(sample).expect("wav sample");
        }
        writer.finalize().expect("wav finalize");

        assert!(audio_source_is_usable(&path.to_string_lossy()));

        let mut bytes = std::fs::read(&path).expect("read wav");
        bytes.truncate(bytes.len() - 2);
        std::fs::write(&path, bytes).expect("truncate wav");
        assert!(!audio_source_is_usable(&path.to_string_lossy()));
    }
}
