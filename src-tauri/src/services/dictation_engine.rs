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

/// What a dispatched action should do, decided purely from state + mode.
///
/// Kept free of `Worker` and `AppHandle` so the frozen mode/signal table
/// (`docs/PHASE_A_HONEST_MVP.md` D10) is unit-testable without a running Tauri app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionPlan {
    Start,
    Stop,
    Cancel,
    Ignore(&'static str),
    Reject(&'static str),
}

fn plan_action(state: EngineState, mode: DictationMode, action: EngineAction) -> ActionPlan {
    match action {
        EngineAction::Pressed => match mode {
            DictationMode::PushToTalk => plan_start(state),
            DictationMode::Toggle => plan_toggle(state),
        },
        EngineAction::Released => match mode {
            DictationMode::PushToTalk => plan_stop(state),
            DictationMode::Toggle => ActionPlan::Ignore("released ignored in toggle mode"),
        },
        EngineAction::Toggle => plan_toggle(state),
        EngineAction::Start => plan_start(state),
        EngineAction::Stop => plan_stop(state),
        EngineAction::Cancel => plan_cancel(state),
    }
}

fn plan_toggle(state: EngineState) -> ActionPlan {
    if state == EngineState::Listening {
        plan_stop(state)
    } else {
        plan_start(state)
    }
}

fn plan_start(state: EngineState) -> ActionPlan {
    match state {
        EngineState::MicCheck => ActionPlan::Reject("Finish microphone check first."),
        EngineState::Processing => ActionPlan::Ignore("already processing"),
        EngineState::Listening => ActionPlan::Ignore("already listening"),
        // Pasted / Error settle straight back into a startable state.
        EngineState::Idle | EngineState::Pasted | EngineState::Error => ActionPlan::Start,
    }
}

fn plan_stop(state: EngineState) -> ActionPlan {
    if state == EngineState::Listening {
        ActionPlan::Stop
    } else {
        ActionPlan::Ignore("not listening")
    }
}

fn plan_cancel(state: EngineState) -> ActionPlan {
    match state {
        EngineState::Listening | EngineState::MicCheck => ActionPlan::Cancel,
        EngineState::Processing => ActionPlan::Reject("cannot cancel while processing"),
        _ => ActionPlan::Reject("no active recording to cancel"),
    }
}

/// How long the worker may block before it must settle a terminal phase back to idle.
/// `None` means nothing is pending and the worker can block indefinitely.
fn settle_wait(state: EngineState, deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    if !matches!(state, EngineState::Pasted | EngineState::Error) {
        return None;
    }
    Some(deadline?.saturating_duration_since(now))
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
        loop {
            // In a terminal phase, wake up when the settle deadline expires so the UI
            // returns to idle on its own instead of waiting for the next user action.
            let cmd = match self.settle_wait() {
                Some(wait) => match rx.recv_timeout(wait) {
                    Ok(cmd) => Some(cmd),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => break,
                },
            };
            self.tick_settle();
            let Some(cmd) = cmd else {
                continue;
            };
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

    fn settle_wait(&self) -> Option<Duration> {
        settle_wait(self.state, self.settle_deadline, Instant::now())
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

        match plan_action(self.state, self.load_mode(), action) {
            ActionPlan::Start => self.enter_listening(),
            ActionPlan::Stop => self.try_stop_fire_and_forget(),
            ActionPlan::Cancel => self.try_cancel_action(),
            ActionPlan::Ignore(reason) => DispatchResult::Ignored { reason },
            ActionPlan::Reject(reason) => DispatchResult::Rejected {
                reason: reason.to_string(),
            },
        }
    }

    fn enter_listening(&mut self) -> DispatchResult {
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
            "Transcribing locally…",
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
            let transcription = transcriber::transcribe(&app, &snapshot.settings, &finished.path)?;
            timer.mark_asr(asr_started.elapsed().as_millis() as u64);
            timer.mark_backend(transcription.backend);
            let raw_text = transcription.text;

            let cleanup_started = Instant::now();
            let cleaned_text = if snapshot.settings.cleanup_enabled {
                cleanup::clean_text(&raw_text, &snapshot.dictionary, &snapshot.snippets)
            } else {
                raw_text.trim().to_string()
            };
            timer.mark_cleanup(cleanup_started.elapsed().as_millis() as u64);

            let preferred = last_hwnd.map(|hwnd| injection::InjectionTarget {
                hwnd,
                process_name: injection::process_name_for(hwnd),
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

    const PTT: DictationMode = DictationMode::PushToTalk;
    const TOGGLE: DictationMode = DictationMode::Toggle;

    /// D10 hard gate: one `Pressed` produces exactly one `Listening` transition.
    /// The second press must not start a competing recording.
    #[test]
    fn one_pressed_yields_one_listening() {
        assert_eq!(
            plan_action(EngineState::Idle, PTT, EngineAction::Pressed),
            ActionPlan::Start
        );
        assert_eq!(
            plan_action(EngineState::Listening, PTT, EngineAction::Pressed),
            ActionPlan::Ignore("already listening")
        );
    }

    /// D10: toggle mode ignores key-up entirely, so holding the chord does not stop it.
    #[test]
    fn toggle_mode_ignores_released() {
        assert_eq!(
            plan_action(EngineState::Listening, TOGGLE, EngineAction::Released),
            ActionPlan::Ignore("released ignored in toggle mode")
        );
        // Push-to-talk, by contrast, stops on the same signal.
        assert_eq!(
            plan_action(EngineState::Listening, PTT, EngineAction::Released),
            ActionPlan::Stop
        );
    }

    #[test]
    fn toggle_starts_from_idle_and_stops_from_listening() {
        assert_eq!(
            plan_action(EngineState::Idle, TOGGLE, EngineAction::Pressed),
            ActionPlan::Start
        );
        assert_eq!(
            plan_action(EngineState::Listening, TOGGLE, EngineAction::Pressed),
            ActionPlan::Stop
        );
        // Tray "toggle" behaves identically in both modes.
        for mode in [PTT, TOGGLE] {
            assert_eq!(
                plan_action(EngineState::Idle, mode, EngineAction::Toggle),
                ActionPlan::Start
            );
            assert_eq!(
                plan_action(EngineState::Listening, mode, EngineAction::Toggle),
                ActionPlan::Stop
            );
        }
    }

    /// Illegal re-entry while the ASR pipeline is running must be ignored, not queued.
    #[test]
    fn processing_ignores_re_entry() {
        for mode in [PTT, TOGGLE] {
            for action in [
                EngineAction::Pressed,
                EngineAction::Start,
                EngineAction::Toggle,
            ] {
                assert_eq!(
                    plan_action(EngineState::Processing, mode, action),
                    ActionPlan::Ignore("already processing"),
                    "{action:?} in {mode:?} should be ignored while processing"
                );
            }
            assert_eq!(
                plan_action(EngineState::Processing, mode, EngineAction::Stop),
                ActionPlan::Ignore("not listening")
            );
        }
    }

    /// D12: mic-check and dictation are mutually exclusive.
    #[test]
    fn mic_check_rejects_dictation() {
        for action in [
            EngineAction::Pressed,
            EngineAction::Start,
            EngineAction::Toggle,
        ] {
            assert_eq!(
                plan_action(EngineState::MicCheck, PTT, action),
                ActionPlan::Reject("Finish microphone check first.")
            );
        }
    }

    #[test]
    fn terminal_states_are_startable_again() {
        for state in [EngineState::Pasted, EngineState::Error, EngineState::Idle] {
            assert_eq!(
                plan_action(state, PTT, EngineAction::Pressed),
                ActionPlan::Start,
                "{state:?} should accept a new recording"
            );
        }
    }

    #[test]
    fn cancel_applies_only_to_active_capture() {
        assert_eq!(
            plan_action(EngineState::Listening, PTT, EngineAction::Cancel),
            ActionPlan::Cancel
        );
        assert_eq!(
            plan_action(EngineState::MicCheck, PTT, EngineAction::Cancel),
            ActionPlan::Cancel
        );
        assert_eq!(
            plan_action(EngineState::Processing, PTT, EngineAction::Cancel),
            ActionPlan::Reject("cannot cancel while processing")
        );
        assert_eq!(
            plan_action(EngineState::Idle, PTT, EngineAction::Cancel),
            ActionPlan::Reject("no active recording to cancel")
        );
    }

    /// The worker must not block indefinitely while a settle is pending, or the
    /// overlay stays stuck on the terminal phase until the next user action.
    #[test]
    fn settle_wait_is_bounded_in_terminal_states() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(1200);

        for state in [EngineState::Pasted, EngineState::Error] {
            assert_eq!(
                settle_wait(state, Some(deadline), now),
                Some(Duration::from_millis(1200)),
                "{state:?} must schedule a wake-up"
            );
        }

        // An already-expired deadline wakes the worker immediately rather than
        // underflowing into a very long wait.
        assert_eq!(
            settle_wait(
                EngineState::Pasted,
                Some(now - Duration::from_secs(1)),
                now
            ),
            Some(Duration::ZERO)
        );

        // Non-terminal states have nothing to settle, so the worker may block.
        for state in [
            EngineState::Idle,
            EngineState::Listening,
            EngineState::Processing,
            EngineState::MicCheck,
        ] {
            assert_eq!(settle_wait(state, Some(deadline), now), None);
        }
        assert_eq!(settle_wait(EngineState::Pasted, None, now), None);
    }
}
