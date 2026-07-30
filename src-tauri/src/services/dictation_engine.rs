use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{
        AsrBackend, DictationMode, DictationPhase, DictationResult, EngineActionAck,
        NativeDictationEvent, RecordingStarted, ShortcutGesture, ShortcutSignal, ShortcutSource,
        StageMetrics, StreamingMetrics, TranscriptSession,
    },
    services::{
        app_state::AppState,
        cleanup, injection,
        live_paste::LivePasteContext,
        metrics::{self, StageTimer},
        polish, recorder, sound_check, transcriber,
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
        gesture: Option<ShortcutGesture>,
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
                gesture: None,
                reply: None,
            })
            .is_ok()
    }

    pub fn dispatch_with_ack(&self, action: EngineAction) -> Result<DispatchResult, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(EngineCmd::Action {
                action,
                gesture: None,
                reply: Some(reply_tx),
            })
            .map_err(|_| "dictation engine is not running".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "dictation engine did not acknowledge action".to_string())
    }

    pub fn dispatch_gesture(&self, action: EngineAction, gesture: ShortcutGesture) -> bool {
        self.tx
            .send(EngineCmd::Action {
                action,
                gesture: Some(gesture),
                reply: None,
            })
            .is_ok()
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

static NEXT_GESTURE_ID: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
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
    last_gesture_id: Option<u64>,
    last_gesture_edge: Option<(u64, ShortcutSignal, u64)>,
    last_registration_generation: u64,
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

/// Dropped streaming audio at or below this is absorbed by the 500 ms chunk
/// overlap and the VAD silence margin; beyond it the streamed transcript is
/// suspect and batch re-transcription is worth the extra latency. Frames are
/// 20 ms, so 12 ≈ 240 ms of lost audio.
const STREAMING_DROP_TOLERANCE_FRAMES: u64 = 12;

fn streaming_drop_exceeds_tolerance(dropped_frames: u64) -> bool {
    dropped_frames > STREAMING_DROP_TOLERANCE_FRAMES
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
                last_gesture_id: None,
                last_gesture_edge: None,
                last_registration_generation: 0,
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
                EngineCmd::Action {
                    action,
                    gesture,
                    reply,
                } => {
                    let result = self.handle_action(action, gesture);
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
                    self.emit_phase(DictationPhase::Idle, None, "Ready.", None, None);
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

    fn handle_action(
        &mut self,
        action: EngineAction,
        gesture: Option<ShortcutGesture>,
    ) -> DispatchResult {
        let state_before = self.state;
        if let Some(gesture) = gesture.as_ref()
            && gesture.registration_generation > 0
            && gesture.registration_generation < self.last_registration_generation
        {
            let result = DispatchResult::Ignored {
                reason: "stale registration generation",
            };
            self.emit_action_ack(gesture, state_before, self.state, &result);
            return result;
        }
        if let Some(gesture) = gesture.as_ref()
            && (self.last_gesture_id == Some(gesture.gesture_id)
                || self
                    .last_gesture_edge
                    .is_some_and(|(generation, signal, received_at)| {
                        generation == gesture.registration_generation
                            && signal == gesture.signal
                            && gesture.received_at_ms >= received_at
                            && gesture.received_at_ms.saturating_sub(received_at) <= 75
                    }))
        {
            let result = DispatchResult::Ignored {
                reason: "duplicate gesture",
            };
            self.emit_action_ack(gesture, state_before, self.state, &result);
            return result;
        }
        if let Some(gesture) = gesture.as_ref() {
            self.last_gesture_id = Some(gesture.gesture_id);
            self.last_gesture_edge = Some((
                gesture.registration_generation,
                gesture.signal,
                gesture.received_at_ms,
            ));
            self.last_registration_generation = self
                .last_registration_generation
                .max(gesture.registration_generation);
        }
        if self.shortcut_test_active() {
            let result = DispatchResult::Ignored {
                reason: "shortcut test active",
            };
            if let Some(gesture) = gesture.as_ref() {
                self.emit_action_ack(gesture, state_before, self.state, &result);
            }
            return result;
        }

        let plan = plan_action(self.state, self.load_mode(), action);
        let result = match plan {
            ActionPlan::Start => self.enter_listening(),
            ActionPlan::Stop => self.try_stop_fire_and_forget(gesture.as_ref(), state_before),
            ActionPlan::Cancel => self.try_cancel_action(),
            ActionPlan::Ignore(reason) => DispatchResult::Ignored { reason },
            ActionPlan::Reject(reason) => DispatchResult::Rejected {
                reason: reason.to_string(),
            },
        };
        if !matches!(plan, ActionPlan::Stop)
            && let Some(gesture) = gesture.as_ref()
        {
            self.emit_action_ack(gesture, state_before, self.state, &result);
        }
        result
    }

    fn emit_action_ack(
        &self,
        gesture: &ShortcutGesture,
        before: EngineState,
        after: EngineState,
        result: &DispatchResult,
    ) {
        let (accepted, reason) = match result {
            DispatchResult::Accepted => (true, None),
            DispatchResult::Ignored { reason } => (false, Some((*reason).to_string())),
            DispatchResult::Rejected { reason } => (false, Some(reason.clone())),
        };
        let ack = EngineActionAck {
            gesture_id: gesture.gesture_id,
            accepted,
            state_before: format!("{before:?}").to_ascii_lowercase(),
            state_after: format!("{after:?}").to_ascii_lowercase(),
            reason,
            acknowledged_at_ms: now_ms(),
        };
        let _ = self.app.emit("atmospeak://shortcut-gesture", ack.clone());
        metrics::emit_runtime(
            &self.app,
            "shortcut-ack",
            format!(
                "gesture={} source={:?} signal={:?} accepted={} state={}->{} latency={}ms generation={}",
                gesture.gesture_id,
                gesture.source,
                gesture.signal,
                ack.accepted,
                ack.state_before,
                ack.state_after,
                ack.acknowledged_at_ms
                    .saturating_sub(gesture.received_at_ms),
                gesture.registration_generation,
            ),
        );
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
                self.emit_phase(DictationPhase::Error, None, error.clone(), None, None);
                DispatchResult::Rejected { reason: error }
            }
        }
    }

    fn try_stop_fire_and_forget(
        &mut self,
        gesture: Option<&ShortcutGesture>,
        state_before: EngineState,
    ) -> DispatchResult {
        if self.state != EngineState::Listening {
            return DispatchResult::Ignored {
                reason: "not listening",
            };
        }
        // Process on this worker thread (not hook). Heavy ASR is spawn_blocking-equivalent:
        // we run pipeline inline on the engine worker thread.
        match self.run_pipeline_from_listening(gesture.map(|gesture| (gesture, state_before))) {
            Ok(_) => DispatchResult::Accepted,
            Err(error) => {
                if self.state != EngineState::Error {
                    self.fail_pipeline(error.clone());
                }
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
        match self.run_pipeline_from_listening(None) {
            Ok(result) => Ok(result),
            Err(error) => {
                if self.state != EngineState::Error {
                    self.fail_pipeline(error.clone());
                }
                Err(error)
            }
        }
    }

    fn fail_pipeline(&mut self, error: String) {
        self.state = EngineState::Error;
        self.active_recording = None;
        self.settle_from_terminal();
        self.emit_phase(DictationPhase::Error, None, error, None, None);
    }

    fn cancel_any(&mut self) -> Result<(), String> {
        match self.state {
            EngineState::Listening => {
                let state = self.app.state::<AppState>();
                state.end_level_stream();
                let _ = state.recorder.cancel();
                state.live_paste.clear();
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
                state.end_level_stream();
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
            .start(settings.microphone_name, None)
            .map_err(|e| e.to_string())?;
        sound_check::start_level_events(&self.app);
        self.state = EngineState::MicCheck;
        // No native-dictation emit for mic-check (D12).
        Ok(())
    }

    fn mic_check_stop(&mut self) -> Result<(), String> {
        if self.state != EngineState::MicCheck {
            return Err("microphone check is not active".to_string());
        }
        let state = self.app.state::<AppState>();
        state.end_level_stream();
        let _ = state.recorder.cancel();
        self.state = EngineState::Idle;
        Ok(())
    }

    fn begin_listening(&self) -> Result<RecordingStarted, String> {
        let state = self.app.state::<AppState>();
        let snapshot = state
            .database
            .lock()
            .snapshot()
            .map_err(|e| e.to_string())?;
        let settings = snapshot.settings;
        let prompt = snapshot
            .dictionary
            .iter()
            .filter(|entry| entry.enabled)
            .flat_map(|entry| [entry.phrase.as_str(), entry.replacement.as_str()])
            .chain(
                snapshot
                    .snippets
                    .iter()
                    .filter(|snippet| snippet.enabled)
                    .map(|snippet| snippet.trigger.as_str()),
            )
            .collect::<Vec<_>>()
            .join(", ");
        let streaming = state.streaming_asr().map(|host| recorder::StreamingStart {
            host,
            prompt,
            profile: settings.transcription_profile,
        });
        let started = state
            .recorder
            .start(settings.microphone_name, streaming)
            .map_err(|e| e.to_string())?;
        state.live_paste.begin_session(
            started.id.clone(),
            LivePasteContext {
                cleanup_enabled: settings.cleanup_enabled,
                dictionary: snapshot.dictionary,
                snippets: snapshot.snippets,
            },
        );
        sound_check::start_level_events(&self.app);
        Ok(started)
    }

    fn capture_target_on_listen(&self) {
        let state = self.app.state::<AppState>();
        if let Some(target) = injection::capture_foreground_target() {
            state.set_last_target_window(Some(target.hwnd));
        }
    }

    fn run_pipeline_from_listening(
        &mut self,
        stop_ack: Option<(&ShortcutGesture, EngineState)>,
    ) -> Result<DictationResult, String> {
        let recording = self.active_recording.clone();
        self.state = EngineState::Processing;

        // Snapshot paste-ready text before teardown so release does not wait on Final.
        let stop_outcome = {
            let state = self.app.state::<AppState>();
            let preview_paste = state.live_paste.take_paste_ready();
            state.end_level_stream();
            let capture_started = Instant::now();
            match state.recorder.stop() {
                Ok(captured) => {
                    let capture_stop_ms = capture_started.elapsed().as_millis() as u64;
                    Ok((preview_paste, captured, capture_stop_ms))
                }
                Err(error) => {
                    state.live_paste.clear();
                    Err(error.to_string())
                }
            }
        };
        let (preview_paste, mut captured, capture_stop_ms) = match stop_outcome {
            Ok(values) => {
                if let Some((gesture, state_before)) = stop_ack {
                    self.emit_action_ack(
                        gesture,
                        state_before,
                        EngineState::Processing,
                        &DispatchResult::Accepted,
                    );
                }
                values
            }
            Err(error) => {
                if let Some((gesture, state_before)) = stop_ack {
                    self.emit_action_ack(
                        gesture,
                        state_before,
                        self.state,
                        &DispatchResult::Rejected {
                            reason: error.clone(),
                        },
                    );
                }
                self.fail_pipeline(error.clone());
                return Err(error);
            }
        };

        let drops_exceeded =
            streaming_drop_exceeds_tolerance(captured.streaming_frames_dropped);
        let preview_ready = preview_paste.as_ref().is_some_and(|preview| {
            preview.session_id == captured.id && !preview.paste_text.trim().is_empty()
        });
        // Paste the cleaned live hypothesis whenever it is non-empty for this
        // session. Wall-clock duration often includes trailing silence after the
        // last spoken word, so a coverage gap does not mean missing speech —
        // requiring coverage was forcing Final → host batch (multi-second paste).
        // Material streaming drops still force Final/batch.
        let take_preview = !drops_exceeded && preview_ready;
        if take_preview {
            if let Some(preview) = preview_paste {
                match self.run_preview_paste_path(
                    recording.clone(),
                    captured,
                    preview,
                    capture_stop_ms,
                ) {
                    Ok(result) => return Ok(result),
                    Err(error) => {
                        // Preview paste failed after mic stop — surface the error.
                        self.app.state::<AppState>().live_paste.clear();
                        self.fail_pipeline(error.clone());
                        return Err(error);
                    }
                }
            }
        }

        // Slow path: no usable live hypothesis yet — finalize then paste.
        self.emit_phase(
            DictationPhase::Finalizing,
            None,
            "Transcribing locally…",
            None,
            None,
        );
        self.app.state::<AppState>().live_paste.clear();

        if let Err(error) = recorder::finalize_capture(&mut captured) {
            if let Some(host) = captured.streaming_host.take() {
                host.cancel_session(&captured.id);
            }
            let _ = std::fs::remove_file(&captured.path);
            let error = error.to_string();
            self.fail_pipeline(error.clone());
            return Err(error);
        }
        // The writer thread has joined, so every surviving frame is already on
        // its way to the sidecar. Ask it to reconcile now: the tail decode
        // overlaps the quality gate and WAV teardown below instead of starting
        // only after them.
        let stop_signaled = match captured.streaming_host.as_ref() {
            Some(host) => match host.request_stop(&captured.id) {
                Ok(()) => true,
                Err(error) => {
                    metrics::emit_runtime(
                        &self.app,
                        "streaming-stop-signal-error",
                        format!("session={} {error}", captured.id),
                    );
                    false
                }
            },
            None => false,
        };
        if let Err(error) = recorder::prepare_for_dictation(&mut captured) {
            if let Some(host) = captured.streaming_host.take() {
                host.cancel_session(&captured.id);
            }
            metrics::emit_runtime(
                &self.app,
                "audio-quality-rejected",
                format!(
                    "session={} duration={}ms reason={error}",
                    captured.id, captured.duration_ms
                ),
            );
            let _ = std::fs::remove_file(&captured.path);
            let error = error.to_string();
            self.fail_pipeline(error.clone());
            return Err(error);
        }

        let state = self.app.state::<AppState>();
        let snapshot = state
            .database
            .lock()
            .snapshot()
            .map_err(|e| e.to_string())?;

        let last_hwnd = state.last_target_window();
        let app = self.app.clone();
        drop(state);

        let pipeline =
            (|| -> Result<(DictationResult, StageMetrics, StreamingMetrics), anyhow::Error> {
                let mut timer = StageTimer::new();
                timer.mark_capture_stop(capture_stop_ms);

                let streaming_host = captured.streaming_host.take();
                let streaming_requested = streaming_host.is_some();
                let streaming_frames_dropped = captured.streaming_frames_dropped;
                let write_started = Instant::now();
                let finished = recorder::finish_recording(captured)?;
                timer.mark_write(write_started.elapsed().as_millis() as u64);

                let mut first_partial_ms = None;
                let mut processed_during_recording_ms = 0;
                let mut tail_audio_ms = finished.duration_ms;
                let mut max_backlog_ms = 0;
                let mut sidecar_frames_dropped = 0;
                let mut fallback_reason = None;
                let asr_started = Instant::now();
                let transcription = if let Some(host) = streaming_host {
                    first_partial_ms = host.first_partial_ms();
                    if !stop_signaled || streaming_drop_exceeds_tolerance(streaming_frames_dropped)
                    {
                        fallback_reason = Some(if !stop_signaled {
                            "streaming host did not accept the stop signal".to_string()
                        } else {
                            format!(
                                "{streaming_frames_dropped} streaming audio frames were dropped"
                            )
                        });
                        host.cancel_session(&finished.id);
                        transcriber::transcribe(&app, &snapshot.settings, &finished.path)?
                    } else {
                        // Prefer streaming finalize so mid-hold commits can shrink
                        // release→paste toward the 500ms SLO. Fall back below if
                        // finalize is empty or fails.
                        match host.await_final(&finished.id, Duration::from_secs(120)) {
                            Ok(finalized) if !finalized.text.trim().is_empty() => {
                                transcriber::Transcription {
                                    text: {
                                        processed_during_recording_ms =
                                            finalized.processed_during_recording_ms;
                                        tail_audio_ms = finalized.tail_audio_ms;
                                        max_backlog_ms = finalized.max_backlog_ms;
                                        sidecar_frames_dropped = finalized.audio_frames_dropped;
                                        finalized.text
                                    },
                                    backend: match host.backend() {
                                        AsrBackend::Vulkan => metrics::ASR_BACKEND_VULKAN,
                                        _ => metrics::ASR_BACKEND_STREAMING_CPU,
                                    },
                                }
                            }
                            Ok(_) => {
                                fallback_reason = Some("streaming result was empty".to_string());
                                metrics::emit_runtime(
                                    &app,
                                    "streaming-asr-fallback",
                                    format!(
                                        "session={} streaming result was empty; using batch fallback",
                                        finished.id
                                    ),
                                );
                                transcriber::transcribe(&app, &snapshot.settings, &finished.path)?
                            }
                            Err(error) => {
                                fallback_reason = Some(error.to_string());
                                metrics::emit_runtime(
                                    &app,
                                    "streaming-asr-fallback",
                                    format!(
                                        "session={} streaming finalization failed; using batch fallback: {error}",
                                        finished.id
                                    ),
                                );
                                transcriber::transcribe(&app, &snapshot.settings, &finished.path)?
                            }
                        }
                    }
                } else {
                    transcriber::transcribe(&app, &snapshot.settings, &finished.path)?
                };
                let asr_ms = asr_started.elapsed().as_millis() as u64;
                timer.mark_asr(asr_ms);
                timer.mark_backend(transcription.backend);
                if streaming_frames_dropped > 0 {
                    metrics::emit_runtime(
                        &app,
                        "streaming-audio-overrun",
                        format!(
                            "session={} audio_frames_dropped={streaming_frames_dropped}",
                            finished.id
                        ),
                    );
                }
                let raw_text = transcription.text;

                let cleanup_started = Instant::now();
                let cleaned_text = if snapshot.settings.cleanup_enabled {
                    cleanup::clean_text(&raw_text, &snapshot.dictionary, &snapshot.snippets)
                } else {
                    raw_text.trim().to_string()
                };
                timer.mark_cleanup(cleanup_started.elapsed().as_millis() as u64);

                let mut polished_text: Option<String> = None;
                let paste_text = match polish::polish_if_enabled(
                    &app,
                    &snapshot.settings,
                    &cleaned_text,
                    polish::AUTO_POLISH_TIMEOUT,
                ) {
                    Ok(Some(outcome)) => {
                        metrics::emit_runtime(
                            &app,
                            "polish-ok",
                            format!(
                                "session={} elapsed_ms={}",
                                finished.id, outcome.elapsed_ms
                            ),
                        );
                        polished_text = Some(outcome.text.clone());
                        outcome.text
                    }
                    Ok(None) => cleaned_text.clone(),
                    Err(error) => {
                        let kind = if polish::is_timeout_error(&error) {
                            "polish-timeout"
                        } else {
                            "polish-fallback"
                        };
                        metrics::emit_runtime(
                            &app,
                            kind,
                            format!(
                                "session={} {}",
                                finished.id,
                                polish::sanitize_message(&error.to_string())
                            ),
                        );
                        cleaned_text.clone()
                    }
                };

                let preferred = last_hwnd.map(|hwnd| injection::InjectionTarget {
                    hwnd,
                    process_name: injection::process_name_for(hwnd),
                });

                let inject_started = Instant::now();
                let injection_result = if snapshot.settings.auto_inject {
                    Some(injection::inject_text(
                        &paste_text,
                        snapshot.settings.restore_clipboard,
                        preferred,
                    )?)
                } else {
                    if let Err(error) = injection::copy_text_to_clipboard(&paste_text) {
                        metrics::emit_runtime(
                            &app,
                            "clipboard-copy-error",
                            format!("session={} {error}", finished.id),
                        );
                    }
                    None
                };
                let paste_ms = inject_started.elapsed().as_millis() as u64;
                timer.mark_inject(paste_ms);

                let session = TranscriptSession {
                    id: finished.id.clone(),
                    raw_text,
                    word_count: paste_text.split_whitespace().count(),
                    cleaned_text,
                    polished_text,
                    prefer_polished: true,
                    audio_path: finished.path.to_string_lossy().to_string(),
                    duration_ms: finished.duration_ms,
                    injected: injection_result
                        .as_ref()
                        .map(|result| result.injected)
                        .unwrap_or(false),
                    source_application: injection_result
                        .as_ref()
                        .and_then(|result| result.target_process_name.clone()),
                    created_at: Utc::now(),
                };

                let backend = match transcription.backend {
                    metrics::ASR_BACKEND_VULKAN => AsrBackend::Vulkan,
                    metrics::ASR_BACKEND_STREAMING_CPU => AsrBackend::Cpu,
                    metrics::ASR_BACKEND_HOST => AsrBackend::Host,
                    _ => AsrBackend::Cli,
                };
                let streaming_metrics = StreamingMetrics {
                    session_id: finished.id.clone(),
                    backend,
                    model_id: snapshot.settings.active_model_id.clone(),
                    first_partial_ms,
                    stop_ack_ms: capture_stop_ms,
                    finalize_ms: asr_ms,
                    paste_ms,
                    processed_during_recording_ms,
                    tail_audio_ms,
                    max_backlog_ms,
                    audio_frames_dropped: streaming_frames_dropped + sidecar_frames_dropped,
                    fallback_reason: if streaming_requested {
                        fallback_reason
                    } else {
                        Some("streaming sidecar unavailable or disabled".to_string())
                    },
                };
                let metrics = timer.finish(finished.id, finished.duration_ms);
                Ok((
                    DictationResult {
                        session,
                        injection: injection_result,
                    },
                    metrics,
                    streaming_metrics,
                ))
            })();

        self.finish_pipeline_result(recording, pipeline)
    }

    fn abandon_captured_recording(&self, mut captured: recorder::CapturedRecording) {
        if let Some(host) = captured.streaming_host.take() {
            host.cancel_session(&captured.id);
        }
        let path = captured.path.clone();
        let _ = recorder::finalize_capture(&mut captured);
        let _ = std::fs::remove_file(path);
        self.app.state::<AppState>().live_paste.clear();
    }

    /// Paste the cleaned live hypothesis immediately; WAV / host teardown run after Pasted.
    fn run_preview_paste_path(
        &mut self,
        recording: Option<RecordingStarted>,
        mut captured: recorder::CapturedRecording,
        preview: crate::services::live_paste::LivePasteSnapshot,
        capture_stop_ms: u64,
    ) -> Result<DictationResult, String> {
        let settings_load = {
            let state = self.app.state::<AppState>();
            state
                .database
                .lock()
                .snapshot()
                .map(|snapshot| {
                    (
                        snapshot.settings.auto_inject,
                        snapshot.settings.restore_clipboard,
                        snapshot.settings.active_model_id.clone(),
                        state.last_target_window(),
                    )
                })
                .map_err(|error| error.to_string())
        };
        let (auto_inject, restore_clipboard, active_model_id, last_hwnd) = match settings_load {
            Ok(values) => values,
            Err(error) => {
                self.abandon_captured_recording(captured);
                return Err(error);
            }
        };
        let paste_text = preview.paste_text;
        let raw_text = preview.raw_text;
        let session_id = captured.id.clone();
        let duration_ms = captured.duration_ms;
        let streaming_frames_dropped = captured.streaming_frames_dropped;
        let audio_path = captured.path.to_string_lossy().to_string();
        let first_partial_ms = captured
            .streaming_host
            .as_ref()
            .and_then(|host| host.first_partial_ms());
        let backend = match captured.streaming_host.as_ref().map(|host| host.backend()) {
            Some(AsrBackend::Vulkan) => metrics::ASR_BACKEND_VULKAN,
            Some(_) => metrics::ASR_BACKEND_STREAMING_CPU,
            None => metrics::ASR_BACKEND_STREAMING_CPU,
        };

        let preferred = last_hwnd.map(|hwnd| injection::InjectionTarget {
            hwnd,
            process_name: injection::process_name_for(hwnd),
        });

        let mut timer = StageTimer::new();
        timer.mark_capture_stop(capture_stop_ms);
        timer.mark_write(0);
        timer.mark_asr(0);
        timer.mark_backend(backend);
        timer.mark_cleanup(0);

        let inject_started = Instant::now();
        let injection_result = if auto_inject {
            match injection::inject_text(&paste_text, restore_clipboard, preferred) {
                Ok(result) => Some(result),
                Err(error) => {
                    if let Some(host) = captured.streaming_host.take() {
                        host.cancel_session(&session_id);
                    }
                    let _ = recorder::finalize_capture(&mut captured);
                    let _ = std::fs::remove_file(&captured.path);
                    self.app.state::<AppState>().live_paste.clear();
                    return Err(error.to_string());
                }
            }
        } else {
            if let Err(error) = injection::copy_text_to_clipboard(&paste_text) {
                metrics::emit_runtime(
                    &self.app,
                    "clipboard-copy-error",
                    format!("session={session_id} {error}"),
                );
            }
            None
        };
        let paste_ms = inject_started.elapsed().as_millis() as u64;
        timer.mark_inject(paste_ms);
        // Stop the stage clock at paste — teardown must not inflate release→paste.
        let stage_metrics = timer.finish(session_id.clone(), duration_ms);

        let mut session = TranscriptSession {
            id: session_id.clone(),
            raw_text,
            word_count: paste_text.split_whitespace().count(),
            cleaned_text: paste_text,
            polished_text: None,
            prefer_polished: true,
            audio_path: audio_path.clone(),
            duration_ms,
            injected: injection_result
                .as_ref()
                .map(|result| result.injected)
                .unwrap_or(false),
            source_application: injection_result
                .as_ref()
                .and_then(|result| result.target_process_name.clone()),
            created_at: Utc::now(),
        };

        let result = DictationResult {
            session: session.clone(),
            injection: injection_result,
        };

        // Surface Pasted/Saved in the UI immediately, but keep EngineState::Processing
        // until teardown finishes so a queued hotkey cannot start/stop an empty hold.
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
        let terminal_phase = if injected {
            DictationPhase::Pasted
        } else if result.injection.is_some() {
            DictationPhase::Error
        } else {
            DictationPhase::Saved
        };
        self.emit_phase(
            terminal_phase.clone(),
            recording,
            message,
            Some(result.clone()),
            Some(stage_metrics.clone()),
        );

        // Deferred teardown: cancel finalize, retain WAV for history, persist session.
        if let Some(host) = captured.streaming_host.take() {
            host.cancel_session(&session_id);
        }
        let recording_path = captured.path.clone();
        match recorder::finalize_capture(&mut captured)
            .and_then(|_| recorder::finish_recording(captured))
        {
            Ok(finished) => {
                session.audio_path = finished.path.to_string_lossy().to_string();
            }
            Err(error) => {
                // Do not persist a history path that points at a missing/incomplete WAV.
                metrics::emit_runtime(
                    &self.app,
                    "preview-paste-teardown-error",
                    format!("session={session_id} {error}"),
                );
                let _ = std::fs::remove_file(&recording_path);
                session.audio_path.clear();
            }
        }

        let streaming_backend = match backend {
            metrics::ASR_BACKEND_VULKAN => AsrBackend::Vulkan,
            metrics::ASR_BACKEND_STREAMING_CPU => AsrBackend::Cpu,
            metrics::ASR_BACKEND_HOST => AsrBackend::Host,
            _ => AsrBackend::Cli,
        };
        let streaming_metrics = StreamingMetrics {
            session_id: session_id.clone(),
            backend: streaming_backend,
            model_id: active_model_id,
            first_partial_ms,
            stop_ack_ms: capture_stop_ms,
            finalize_ms: 0,
            paste_ms,
            processed_during_recording_ms: duration_ms,
            tail_audio_ms: 0,
            max_backlog_ms: 0,
            audio_frames_dropped: streaming_frames_dropped,
            fallback_reason: None,
        };
        let state = self.app.state::<AppState>();
        if let Err(error) = state.database.lock().insert_session(&session) {
            metrics::emit_runtime(&self.app, "session-save-error", error.to_string());
        }
        metrics::emit_stage_metrics(&self.app, &stage_metrics);
        metrics::emit_streaming_metrics(&self.app, &streaming_metrics);
        state.live_paste.clear();
        drop(state);

        self.state = if matches!(terminal_phase, DictationPhase::Error) {
            EngineState::Error
        } else {
            EngineState::Pasted
        };
        self.active_recording = None;
        self.settle_from_terminal();
        Ok(DictationResult {
            session,
            injection: result.injection,
        })
    }

    fn finish_pipeline_result(
        &mut self,
        recording: Option<RecordingStarted>,
        pipeline: Result<(DictationResult, StageMetrics, StreamingMetrics), anyhow::Error>,
    ) -> Result<DictationResult, String> {
        let state = self.app.state::<AppState>();
        match pipeline {
            Ok((result, metrics, streaming_metrics)) => {
                if let Err(error) = state.database.lock().insert_session(&result.session) {
                    metrics::emit_runtime(&self.app, "session-save-error", error.to_string());
                }
                metrics::emit_stage_metrics(&self.app, &metrics);
                metrics::emit_streaming_metrics(&self.app, &streaming_metrics);

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
                    self.state = EngineState::Pasted;
                    self.emit_phase(
                        DictationPhase::Saved,
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
        let _ = self
            .app
            .emit("wind-speak://native-dictation", event.clone());
        let _ = self.app.emit("atmospeak://native-dictation", event);
    }
}

/// Route shortcut string payloads into the engine (single dispatch path).
pub fn route_shortcut_payload(app: &AppHandle, payload: &str) {
    route_shortcut_payload_from(app, payload, ShortcutSource::LowLevelHook, 0);
}

pub fn route_shortcut_payload_from(
    app: &AppHandle,
    payload: &str,
    source: ShortcutSource,
    registration_generation: u64,
) {
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
    let (action, signal) = match payload {
        "pressed" => (EngineAction::Pressed, ShortcutSignal::Pressed),
        "released" => (EngineAction::Released, ShortcutSignal::Released),
        "toggle" => (EngineAction::Toggle, ShortcutSignal::Toggle),
        "cancel" => (EngineAction::Cancel, ShortcutSignal::Cancel),
        _ => return,
    };
    let gesture = ShortcutGesture {
        gesture_id: NEXT_GESTURE_ID.fetch_add(1, Ordering::Relaxed),
        registration_generation,
        signal,
        source,
        received_at_ms: now_ms(),
    };
    let _ = engine.dispatch_gesture(action, gesture);
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

    /// Micro-drops are tolerated; material loss must force the batch path.
    #[test]
    fn streaming_drop_tolerance_bounds_fallback() {
        assert!(!streaming_drop_exceeds_tolerance(0));
        assert!(!streaming_drop_exceeds_tolerance(STREAMING_DROP_TOLERANCE_FRAMES));
        assert!(streaming_drop_exceeds_tolerance(STREAMING_DROP_TOLERANCE_FRAMES + 1));
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
            settle_wait(EngineState::Pasted, Some(now - Duration::from_secs(1)), now),
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
