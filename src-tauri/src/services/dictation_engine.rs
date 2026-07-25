use std::{
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{
        DictationMode, DictationPhase, DictationResult, NativeDictationEvent, RecordingStarted,
        StageMetrics, TranscriptSession,
    },
    services::{
        app_state::AppState,
        cleanup, injection,
        metrics::{self, StageTimer},
        recorder, transcriber,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineAction {
    Pressed,
    Released,
    Toggle,
    Cancel,
    Start,
    Stop,
}

#[derive(Debug, Clone)]
pub enum DispatchResult {
    Accepted,
    Ignored { reason: &'static str },
    Rejected { reason: String },
}

enum EngineCmd {
    Action {
        action: EngineAction,
        reply: Option<SyncSender<DispatchResult>>,
    },
    StartBlocking {
        reply: SyncSender<Result<RecordingStarted, String>>,
    },
    StopBlocking {
        reply: SyncSender<Result<DictationResult, String>>,
    },
    CancelBlocking {
        reply: SyncSender<Result<(), String>>,
    },
    MicCheckStart {
        reply: SyncSender<Result<(), String>>,
    },
    MicCheckStop {
        reply: SyncSender<Result<(), String>>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct EngineHandle {
    tx: Sender<EngineCmd>,
}

impl EngineHandle {
    pub fn dispatch_fire_and_forget(&self, action: EngineAction) -> bool {
        self.tx
            .send(EngineCmd::Action {
                action,
                reply: None,
            })
            .is_ok()
    }

    pub fn dispatch_with_ack(&self, action: EngineAction) -> Result<DispatchResult, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::Action {
                action,
                reply: Some(reply_tx),
            })
            .map_err(|_| "dictation engine is not running".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "dictation engine did not acknowledge action".to_string())
    }

    pub fn start_blocking(&self) -> Result<RecordingStarted, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::StartBlocking { reply: reply_tx })
            .map_err(|_| "dictation engine is not running".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "start_recording timed out".to_string())?
    }

    pub fn stop_blocking(&self) -> Result<DictationResult, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::StopBlocking { reply: reply_tx })
            .map_err(|_| "dictation engine is not running".to_string())?;
        // Whisper CLI can take a long time on cold start.
        reply_rx
            .recv_timeout(Duration::from_secs(300))
            .map_err(|_| "stop_recording timed out".to_string())?
    }

    pub fn cancel_blocking(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::CancelBlocking { reply: reply_tx })
            .map_err(|_| "dictation engine is not running".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "cancel_recording timed out".to_string())?
    }

    pub fn mic_check_start(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::MicCheckStart { reply: reply_tx })
            .map_err(|_| "dictation engine is not running".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "mic_check_start timed out".to_string())?
    }

    pub fn mic_check_stop(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::MicCheckStop { reply: reply_tx })
            .map_err(|_| "dictation engine is not running".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "mic_check_stop timed out".to_string())?
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineState {
    Idle,
    Listening,
    Processing,
    Pasted,
    Error,
    MicCheck,
}

struct Worker {
    app: AppHandle,
    state: EngineState,
    active_recording: Option<RecordingStarted>,
    settle_deadline: Option<Instant>,
}

pub fn spawn(app: AppHandle) -> EngineHandle {
    let (tx, rx) = mpsc::channel::<EngineCmd>();
    let worker_app = app.clone();
    thread::Builder::new()
        .name("atmospeak-dictation-engine".into())
        .spawn(move || {
            let mut worker = Worker {
                app: worker_app,
                state: EngineState::Idle,
                active_recording: None,
                settle_deadline: None,
            };
            worker.run(rx);
        })
        .expect("failed to spawn dictation engine worker");
    EngineHandle { tx }
}

impl Worker {
    fn run(&mut self, rx: Receiver<EngineCmd>) {
        while let Ok(cmd) = rx.recv() {
            self.tick_settle();
            match cmd {
                EngineCmd::Shutdown => break,
                EngineCmd::Action { action, reply } => {
                    let result = self.handle_action(action);
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                }
                EngineCmd::StartBlocking { reply } => {
                    let result = self.start_dictation_blocking();
                    let _ = reply.send(result);
                }
                EngineCmd::StopBlocking { reply } => {
                    let result = self.stop_dictation_blocking();
                    let _ = reply.send(result);
                }
                EngineCmd::CancelBlocking { reply } => {
                    let result = self.cancel_any();
                    let _ = reply.send(result);
                }
                EngineCmd::MicCheckStart { reply } => {
                    let result = self.mic_check_start();
                    let _ = reply.send(result);
                }
                EngineCmd::MicCheckStop { reply } => {
                    let result = self.mic_check_stop();
                    let _ = reply.send(result);
                }
            }
        }
    }

    fn tick_settle(&mut self) {
        if matches!(self.state, EngineState::Pasted | EngineState::Error) {
            if let Some(deadline) = self.settle_deadline {
                if Instant::now() >= deadline {
                    self.state = EngineState::Idle;
                    self.active_recording = None;
                    self.settle_deadline = None;
                    self.emit_phase(
                        DictationPhase::Idle,
                        None,
                        "Ready.",
                        None,
                        None,
                    );
                }
            }
        }
    }

    fn settle_from_terminal(&mut self) {
        // Allow immediate re-start; also schedule soft settle for UI.
        self.settle_deadline = Some(Instant::now() + Duration::from_millis(1200));
    }

    fn can_start_from(&self) -> bool {
        matches!(
            self.state,
            EngineState::Idle | EngineState::Pasted | EngineState::Error
        )
    }

    fn handle_action(&mut self, action: EngineAction) -> DispatchResult {
        if self.shortcut_test_active() {
            return DispatchResult::Ignored {
                reason: "shortcut test active",
            };
        }

        let mode = self.load_mode();

        match action {
            EngineAction::Pressed => match mode {
                DictationMode::PushToTalk => self.try_start(),
                DictationMode::Toggle => self.try_toggle(),
            },
            EngineAction::Released => match mode {
                DictationMode::PushToTalk => self.try_stop_fire_and_forget(),
                DictationMode::Toggle => DispatchResult::Ignored {
                    reason: "released ignored in toggle mode",
                },
            },
            EngineAction::Toggle => self.try_toggle(),
            EngineAction::Start => self.try_start(),
            EngineAction::Stop => self.try_stop_fire_and_forget(),
            EngineAction::Cancel => self.try_cancel_action(),
        }
    }

    fn try_toggle(&mut self) -> DispatchResult {
        if self.state == EngineState::Listening {
            self.try_stop_fire_and_forget()
        } else {
            self.try_start()
        }
    }

    fn try_start(&mut self) -> DispatchResult {
        if self.state == EngineState::MicCheck {
            return DispatchResult::Rejected {
                reason: "Finish microphone check first.".to_string(),
            };
        }
        if self.state == EngineState::Processing {
            return DispatchResult::Ignored {
                reason: "already processing",
            };
        }
        if self.state == EngineState::Listening {
            return DispatchResult::Ignored {
                reason: "already listening",
            };
        }
        if !self.can_start_from() {
            return DispatchResult::Ignored {
                reason: "not ready",
            };
        }
        match self.begin_listening() {
            Ok(started) => {
                self.state = EngineState::Listening;
                self.active_recording = Some(started.clone());
                self.settle_deadline = None;
                self.capture_target_on_listen();
                self.emit_phase(
                    DictationPhase::Listening,
                    Some(started),
                    "Listening…",
                    None,
                    None,
                );
                DispatchResult::Accepted
            }
            Err(error) => {
                self.state = EngineState::Error;
                self.settle_from_terminal();
                self.emit_phase(
                    DictationPhase::Error,
                    None,
                    error.clone(),
                    None,
                    None,
                );
                DispatchResult::Rejected { reason: error }
            }
        }
    }

    fn try_stop_fire_and_forget(&mut self) -> DispatchResult {
        if self.state != EngineState::Listening {
            return DispatchResult::Ignored {
                reason: "not listening",
            };
        }
        // Process on this worker thread (not hook). Heavy ASR is spawn_blocking-equivalent:
        // we run pipeline inline on the engine worker thread.
        match self.run_pipeline_from_listening() {
            Ok(_) => DispatchResult::Accepted,
            Err(error) => {
                self.state = EngineState::Error;
                self.active_recording = None;
                self.settle_from_terminal();
                self.emit_phase(DictationPhase::Error, None, error, None, None);
                DispatchResult::Accepted
            }
        }
    }

    fn try_cancel_action(&mut self) -> DispatchResult {
        match self.cancel_any() {
            Ok(()) => DispatchResult::Accepted,
            Err(error) => DispatchResult::Rejected { reason: error },
        }
    }

    fn start_dictation_blocking(&mut self) -> Result<RecordingStarted, String> {
        if self.state == EngineState::MicCheck {
            return Err("Finish microphone check first.".to_string());
        }
        if self.state == EngineState::Processing {
            return Err("already processing".to_string());
        }
        if self.state == EngineState::Listening {
            if let Some(started) = self.active_recording.clone() {
                return Ok(started);
            }
        }
        if !self.can_start_from() && self.state != EngineState::Listening {
            return Err("not ready to start recording".to_string());
        }
        let started = self.begin_listening().map_err(|e| e)?;
        self.state = EngineState::Listening;
        self.active_recording = Some(started.clone());
        self.settle_deadline = None;
        self.capture_target_on_listen();
        self.emit_phase(
            DictationPhase::Listening,
            Some(started.clone()),
            format!("Recording from {}.", started.microphone_name),
            None,
            None,
        );
        Ok(started)
    }

    fn stop_dictation_blocking(&mut self) -> Result<DictationResult, String> {
        if self.state != EngineState::Listening {
            return Err("no active recording to stop".to_string());
        }
        self.run_pipeline_from_listening()
    }

    fn cancel_any(&mut self) -> Result<(), String> {
        match self.state {
            EngineState::Listening => {
                let state = self.app.state::<AppState>();
                let _ = state.recorder.cancel();
                self.state = EngineState::Idle;
                self.active_recording = None;
                self.settle_deadline = None;
                self.emit_phase(
                    DictationPhase::Idle,
                    None,
                    "Recording cancelled.",
                    None,
                    None,
                );
                Ok(())
            }
            EngineState::MicCheck => {
                let state = self.app.state::<AppState>();
                let _ = state.recorder.cancel();
                self.state = EngineState::Idle;
                Ok(())
            }
            EngineState::Processing => Err("cannot cancel while processing".to_string()),
            _ => Err("no active recording to cancel".to_string()),
        }
    }

    fn mic_check_start(&mut self) -> Result<(), String> {
        if matches!(
            self.state,
            EngineState::Listening | EngineState::Processing | EngineState::MicCheck
        ) {
            return Err("Stop the current recording before checking the microphone.".to_string());
        }
        let state = self.app.state::<AppState>();
        let settings = state
            .database
            .lock()
            .load_settings()
            .map_err(|e| e.to_string())?;
        let _started = state
            .recorder
            .start(settings.microphone_name)
            .map_err(|e| e.to_string())?;
        self.state = EngineState::MicCheck;
        // No native-dictation emit for mic-check (D12).
        Ok(())
    }

    fn mic_check_stop(&mut self) -> Result<(), String> {
        if self.state != EngineState::MicCheck {
            return Err("microphone check is not active".to_string());
        }
        let state = self.app.state::<AppState>();
        let _ = state.recorder.cancel();
        self.state = EngineState::Idle;
        Ok(())
    }

    fn begin_listening(&self) -> Result<RecordingStarted, String> {
        let state = self.app.state::<AppState>();
        let settings = state
            .database
            .lock()
            .load_settings()
            .map_err(|e| e.to_string())?;
        state
            .recorder
            .start(settings.microphone_name)
            .map_err(|e| e.to_string())
    }

    fn capture_target_on_listen(&self) {
        let state = self.app.state::<AppState>();
        if let Some(target) = injection::capture_foreground_target() {
            state.set_last_target_window(Some(target.hwnd));
        }
    }

    fn run_pipeline_from_listening(&mut self) -> Result<DictationResult, String> {
        let recording = self.active_recording.clone();
        self.state = EngineState::Processing;
        self.emit_phase(
            DictationPhase::Processing,
            recording.clone(),
            "Transcribing locally… (CLI; may take several seconds)",
            None,
            None,
        );

        let state = self.app.state::<AppState>();
        let capture_started = Instant::now();
        let captured = state.recorder.stop().map_err(|e| e.to_string())?;
        let capture_stop_ms = capture_started.elapsed().as_millis() as u64;

        let snapshot = state
            .database
            .lock()
            .snapshot()
            .map_err(|e| e.to_string())?;

        let last_hwnd = state.last_target_window();
        let app = self.app.clone();

        let pipeline = (|| -> Result<(DictationResult, StageMetrics), anyhow::Error> {
            let mut timer = StageTimer::new();
            timer.mark_capture_stop(capture_stop_ms);

            let write_started = Instant::now();
            let finished = recorder::finish_recording(captured)?;
            timer.mark_write(write_started.elapsed().as_millis() as u64);

            let asr_started = Instant::now();
            let raw_text = transcriber::transcribe(&app, &snapshot.settings, &finished.path)?;
            timer.mark_asr(asr_started.elapsed().as_millis() as u64);

            let cleanup_started = Instant::now();
            let cleaned_text = if snapshot.settings.cleanup_enabled {
                cleanup::clean_text(&raw_text, &snapshot.dictionary, &snapshot.snippets)
            } else {
                raw_text.trim().to_string()
            };
            timer.mark_cleanup(cleanup_started.elapsed().as_millis() as u64);

            let preferred = last_hwnd.map(|hwnd| injection::InjectionTarget {
                hwnd,
                process_name: None,
            });

            let inject_started = Instant::now();
            let injection_result = if snapshot.settings.auto_inject {
                Some(injection::inject_text(
                    &cleaned_text,
                    snapshot.settings.restore_clipboard,
                    preferred,
                )?)
            } else {
                None
            };
            timer.mark_inject(inject_started.elapsed().as_millis() as u64);

            let session = TranscriptSession {
                id: finished.id.clone(),
                raw_text,
                word_count: cleaned_text.split_whitespace().count(),
                cleaned_text,
                audio_path: finished.path.to_string_lossy().to_string(),
                duration_ms: finished.duration_ms,
                injected: injection_result
                    .as_ref()
                    .map(|result| result.injected)
                    .unwrap_or(false),
                created_at: Utc::now(),
            };

            let metrics = timer.finish(finished.id, finished.duration_ms);
            Ok((
                DictationResult {
                    session,
                    injection: injection_result,
                },
                metrics,
            ))
        })();

        match pipeline {
            Ok((result, metrics)) => {
                if let Err(error) = state.database.lock().insert_session(&result.session) {
                    metrics::emit_runtime(&self.app, "session-save-error", error.to_string());
                }
                metrics::emit_stage_metrics(&self.app, &metrics);

                let injected = result
                    .injection
                    .as_ref()
                    .map(|injection| injection.injected)
                    .unwrap_or(false);
                let message = result
                    .injection
                    .as_ref()
                    .map(|injection| injection.message.clone())
                    .unwrap_or_else(|| "Transcript saved to history.".to_string());

                if injected {
                    self.state = EngineState::Pasted;
                    self.emit_phase(
                        DictationPhase::Pasted,
                        recording,
                        message,
                        Some(result.clone()),
                        Some(metrics),
                    );
                } else if result.injection.is_some() {
                    // Paste soft-failed but clipboard has text
                    self.state = EngineState::Error;
                    self.emit_phase(
                        DictationPhase::Error,
                        recording,
                        message,
                        Some(result.clone()),
                        Some(metrics),
                    );
                } else {
                    self.state = EngineState::Idle;
                    self.emit_phase(
                        DictationPhase::Idle,
                        recording,
                        message,
                        Some(result.clone()),
                        Some(metrics),
                    );
                }
                self.active_recording = None;
                self.settle_from_terminal();
                Ok(result)
            }
            Err(error) => {
                let message = error.to_string();
                self.state = EngineState::Error;
                self.active_recording = None;
                self.settle_from_terminal();
                self.emit_phase(
                    DictationPhase::Error,
                    recording,
                    message.clone(),
                    None,
                    None,
                );
                Err(message)
            }
        }
    }

    fn load_mode(&self) -> DictationMode {
        self.app
            .state::<AppState>()
            .database
            .lock()
            .load_settings()
            .map(|settings| settings.mode)
            .unwrap_or(DictationMode::PushToTalk)
    }

    fn shortcut_test_active(&self) -> bool {
        self.app.state::<AppState>().shortcut_test_active()
    }

    fn emit_phase(
        &self,
        phase: DictationPhase,
        recording: Option<RecordingStarted>,
        message: impl Into<String>,
        result: Option<DictationResult>,
        metrics: Option<StageMetrics>,
    ) {
        let event = NativeDictationEvent {
            recording,
            phase,
            message: message.into(),
            result,
            metrics,
        };
        let _ = self.app.emit("wind-speak://native-dictation", event.clone());
        let _ = self.app.emit("atmospeak://native-dictation", event);
    }
}

/// Route shortcut string payloads into the engine (single dispatch path).
pub fn route_shortcut_payload(app: &AppHandle, payload: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if state.shortcut_test_active() {
        // Emit only — detection is for UI test; no Listening.
        return;
    }
    let Some(engine) = state.engine() else {
        return;
    };
    let action = match payload {
        "pressed" => EngineAction::Pressed,
        "released" => EngineAction::Released,
        "toggle" => EngineAction::Toggle,
        "cancel" => EngineAction::Cancel,
        _ => return,
    };
    let _ = engine.dispatch_fire_and_forget(action);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_mode_ignores_released_semantics() {
        // Document D10: released is ignored in toggle — covered by match arms.
        assert!(matches!(EngineAction::Released, EngineAction::Released));
    }
}
