use std::time::Instant;

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{RuntimeEvent, StageMetrics},
    services::app_state::AppState,
};

/// One-shot `whisper-cli.exe` per utterance (Phase A, and the Phase B fallback).
pub const ASR_BACKEND_CLI: &str = "cli";
/// Resident `whisper-server.exe` with the model kept warm (Phase B).
pub const ASR_BACKEND_HOST: &str = "host";

pub struct StageTimer {
    started: Instant,
    capture_stop_ms: u64,
    write_ms: u64,
    asr_ms: u64,
    cleanup_ms: u64,
    inject_ms: u64,
    asr_backend: &'static str,
}

impl StageTimer {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            capture_stop_ms: 0,
            write_ms: 0,
            asr_ms: 0,
            cleanup_ms: 0,
            inject_ms: 0,
            asr_backend: ASR_BACKEND_CLI,
        }
    }

    pub fn mark_backend(&mut self, backend: &'static str) {
        self.asr_backend = backend;
    }

    pub fn mark_capture_stop(&mut self, ms: u64) {
        self.capture_stop_ms = ms;
    }

    pub fn mark_write(&mut self, ms: u64) {
        self.write_ms = ms;
    }

    pub fn mark_asr(&mut self, ms: u64) {
        self.asr_ms = ms;
    }

    pub fn mark_cleanup(&mut self, ms: u64) {
        self.cleanup_ms = ms;
    }

    pub fn mark_inject(&mut self, ms: u64) {
        self.inject_ms = ms;
    }

    pub fn finish(self, session_id: String, audio_duration_ms: u64) -> StageMetrics {
        StageMetrics {
            session_id,
            capture_stop_ms: self.capture_stop_ms,
            write_ms: self.write_ms,
            asr_ms: self.asr_ms,
            cleanup_ms: self.cleanup_ms,
            inject_ms: self.inject_ms,
            total_ms: self.started.elapsed().as_millis() as u64,
            asr_backend: self.asr_backend.to_string(),
            audio_duration_ms,
        }
    }
}

pub fn emit_stage_metrics(app: &AppHandle, metrics: &StageMetrics) {
    let _ = app.emit("wind-speak://stage-metrics", metrics.clone());
    let _ = app.emit("atmospeak://stage-metrics", metrics.clone());
    if let Some(state) = app.try_state::<AppState>() {
        state.set_last_metrics(metrics.clone());
        state.record_event(RuntimeEvent {
            created_at: Utc::now(),
            kind: "stage-metrics".to_string(),
            message: format!(
                "session {} total={}ms asr={}ms backend={}",
                metrics.session_id, metrics.total_ms, metrics.asr_ms, metrics.asr_backend
            ),
        });
    }
    eprintln!(
        "atmospeak stage-metrics session={} capture_stop={} write={} asr={} cleanup={} inject={} total={} audio={}",
        metrics.session_id,
        metrics.capture_stop_ms,
        metrics.write_ms,
        metrics.asr_ms,
        metrics.cleanup_ms,
        metrics.inject_ms,
        metrics.total_ms,
        metrics.audio_duration_ms
    );
}

pub fn emit_runtime(app: &AppHandle, kind: &str, message: impl Into<String>) {
    let event = RuntimeEvent {
        created_at: Utc::now(),
        kind: kind.to_string(),
        message: message.into(),
    };
    let _ = app.emit("wind-speak://runtime-event", event.clone());
    let _ = app.emit("atmospeak://runtime-event", event.clone());
    if let Some(state) = app.try_state::<AppState>() {
        state.record_event(event);
    }
}
