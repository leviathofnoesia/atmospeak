use std::{
    sync::{
        Arc, Mutex,
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
        NativeDictationEvent, RecordingStarted, RuntimeEvent, ShortcutGesture, ShortcutSignal,
        ShortcutSource, StageMetrics, StreamingMetrics, TranscriptSession, TranscriptionProfile,
    },
    services::{
        app_state::AppState,
        dictation_polish::{self, HistoryPolishWorker},
        injection,
        text_processing::DictationTextContext,
        metrics::{self, StageTimer},
        polish, recorder, sound_check, transcriber,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineAction {
    Pressed,
    Released,
    EditSelection,
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
        queued_at: Instant,
    },
    StartBlocking {
        reply: SyncSender<Result<RecordingStarted, String>>,
    },
    StopBlocking {
        reply: SyncSender<Result<DictationResult, String>>,
        queued_at: Instant,
    },
    CancelBlocking {
        reply: Option<SyncSender<Result<(), String>>>,
    },
    MicCheckStart {
        reply: SyncSender<Result<(), String>>,
    },
    MicCheckStop {
        reply: SyncSender<Result<(), String>>,
    },
}

#[derive(Clone)]
pub struct EngineHandle {
    tx: Sender<EngineCmd>,
    /// Set directly by the caller so cancellation does not wait behind a
    /// blocking inference/finalization call on the engine worker.
    cancel_requested: Arc<std::sync::atomic::AtomicBool>,
    delivery_boundary: Arc<DeliveryBoundary>,
}

/// Serializes the cancellation decision with the first externally visible
/// delivery. A cancellation that wins this lock prevents delivery; a delivery
/// that wins it makes later cancellation an explicit rejection.
struct DeliveryBoundary {
    committed: Mutex<bool>,
}

impl DeliveryBoundary {
    fn new() -> Self {
        Self {
            committed: Mutex::new(false),
        }
    }

    fn reset(&self) {
        let mut committed = self
            .committed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *committed = false;
    }

    fn request_cancel(
        &self,
        cancel_requested: &std::sync::atomic::AtomicBool,
    ) -> Result<(), &'static str> {
        let committed = self
            .committed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *committed {
            return Err("dictation delivery already started");
        }
        cancel_requested.store(true, Ordering::Release);
        Ok(())
    }

    fn begin_delivery(&self, cancel_requested: &std::sync::atomic::AtomicBool) -> bool {
        let mut committed = self
            .committed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if cancel_requested.load(Ordering::Acquire) {
            return false;
        }
        *committed = true;
        true
    }

    fn is_committed(&self) -> bool {
        *self
            .committed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl EngineHandle {
    pub fn dispatch_fire_and_forget(&self, action: EngineAction) -> bool {
        if action == EngineAction::Cancel {
            let _ = self
                .delivery_boundary
                .request_cancel(self.cancel_requested.as_ref());
        }
        self.tx
            .send(EngineCmd::Action {
                action,
                gesture: None,
                reply: None,
                queued_at: Instant::now(),
            })
            .is_ok()
    }

    pub fn dispatch_gesture(&self, action: EngineAction, gesture: ShortcutGesture) -> bool {
        if action == EngineAction::Cancel {
            let _ = self
                .delivery_boundary
                .request_cancel(self.cancel_requested.as_ref());
        }
        self.tx
            .send(EngineCmd::Action {
                action,
                gesture: Some(gesture),
                reply: None,
                queued_at: Instant::now(),
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
            .send(EngineCmd::StopBlocking {
                reply: reply_tx,
                queued_at: Instant::now(),
            })
            .map_err(|_| "dictation engine is not running".to_string())?;
        // Whisper CLI can take a long time on cold start.
        reply_rx
            .recv_timeout(Duration::from_secs(300))
            .map_err(|_| "stop_recording timed out".to_string())?
    }

    pub fn cancel_blocking(&self) -> Result<(), String> {
        // This store is intentionally independent from the command queue. The
        // worker may be inside ASR or polish and therefore unable to receive a
        // queued command for a while; the pipeline checks this signal before
        // every irreversible stage and before injection.
        self.delivery_boundary
            .request_cancel(self.cancel_requested.as_ref())
            .map_err(str::to_string)?;
        self.tx
            .send(EngineCmd::CancelBlocking {
                // A processing cancellation is acknowledged by the atomic
                // signal above. The queued arm still handles an active capture
                // synchronously when the worker is idle enough to receive it.
                reply: None,
            })
            .map_err(|_| "dictation engine is not running".to_string())?;
        Ok(())
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
    cancel_requested: Arc<std::sync::atomic::AtomicBool>,
    delivery_boundary: Arc<DeliveryBoundary>,
    state: EngineState,
    active_recording: Option<RecordingStarted>,
    active_settings: Option<crate::models::AppSettings>,
    active_cleanup: Option<DictationTextContext>,
    history_polish: Option<HistoryPolishWorker>,
    selected_edit: Option<crate::services::selected_text::SelectedText>,
    settle_deadline: Option<Instant>,
    last_gesture_id: Option<u64>,
    last_gesture_edge: Option<(u64, ShortcutSignal, u64)>,
    last_registration_generation: u64,
    request_started: Instant,
    pending_diagnostics: Vec<RuntimeEvent>,
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
        EngineAction::Toggle | EngineAction::EditSelection => plan_toggle(state),
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
        EngineState::Listening | EngineState::Processing | EngineState::MicCheck => {
            ActionPlan::Cancel
        }
        _ => ActionPlan::Reject("no active recording to cancel"),
    }
}

/// External injection requires a drop-free source. Even a single missing frame
/// can remove the final word while a live transcript still looks plausible.
fn streaming_drop_requires_fallback(dropped_frames: u64) -> bool {
    dropped_frames > 0
}

fn streaming_final_is_releaseable(text: &str, dropped_frames: u64) -> bool {
    !text.trim().is_empty() && !streaming_drop_requires_fallback(dropped_frames)
}
/// The one place a transcript enters the user's machine. Every source goes
/// through it, so auto-inject, clipboard fallback, target restore, and the paste
/// clock cannot drift apart between them. Returns the injection outcome (`None`
/// when auto-inject is off and the text was only copied) and the inject cost.
fn inject_transcript(
    app: &AppHandle,
    session_id: &str,
    paste_text: &str,
    auto_inject: bool,
    restore_clipboard: bool,
    last_hwnd: Option<isize>,
) -> Result<(Option<crate::models::InjectionResult>, u64), anyhow::Error> {
    let preferred = last_hwnd.map(|hwnd| injection::InjectionTarget {
        hwnd,
        process_name: injection::process_name_for(hwnd),
    });
    let inject_started = Instant::now();
    let injected = if auto_inject {
        Some(injection::inject_text(
            paste_text,
            restore_clipboard,
            preferred,
        )?)
    } else {
        if let Err(error) = injection::copy_text_to_clipboard(paste_text) {
            metrics::emit_runtime(
                app,
                "clipboard-copy-error",
                format!("session={session_id} {error}"),
            );
        }
        None
    };
    Ok((injected, inject_started.elapsed().as_millis() as u64))
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
    let cancel_requested = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel_requested = cancel_requested.clone();
    let delivery_boundary = Arc::new(DeliveryBoundary::new());
    let worker_delivery_boundary = delivery_boundary.clone();
    let worker_app = app.clone();
    thread::Builder::new()
        .name("atmospeak-dictation-engine".into())
        .spawn(move || {
            let history_polish = HistoryPolishWorker::spawn(worker_app.clone())
                .map_err(|error| metrics::emit_runtime(&worker_app, "polish-worker-error", error.to_string()))
                .ok();
            let mut worker = Worker {
                app: worker_app,
                cancel_requested: worker_cancel_requested,
                delivery_boundary: worker_delivery_boundary,
                state: EngineState::Idle,
                active_recording: None,
                active_settings: None,
                active_cleanup: None,
                history_polish,
                selected_edit: None,
                settle_deadline: None,
                last_gesture_id: None,
                last_gesture_edge: None,
                last_registration_generation: 0,
                request_started: Instant::now(),
                pending_diagnostics: Vec::new(),
            };
            worker.run(rx);
        })
        .expect("failed to spawn dictation engine worker");
    EngineHandle {
        tx,
        cancel_requested,
        delivery_boundary,
    }
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
                EngineCmd::Action {
                    action,
                    gesture,
                    reply,
                    queued_at,
                } => {
                    self.request_started = queued_at;
                    let result = self.handle_action(action, gesture);
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                }
                EngineCmd::StartBlocking { reply } => {
                    let result = self.start_dictation_blocking();
                    let _ = reply.send(result);
                }
                EngineCmd::StopBlocking { reply, queued_at } => {
                    self.request_started = queued_at;
                    let result = self.stop_dictation_blocking();
                    let _ = reply.send(result);
                }
                EngineCmd::CancelBlocking { reply } => {
                    let result = self.cancel_any();
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
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
            if !self.pending_diagnostics.is_empty() {
                metrics::emit_runtime_events(
                    &self.app,
                    std::mem::take(&mut self.pending_diagnostics),
                );
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
        if let Some(gesture) = gesture.as_ref() {
            self.pending_diagnostics.push(RuntimeEvent {
                created_at: Utc::now(),
                kind: "shortcut-gesture".into(),
                message: format!(
                    "gesture={} source={:?} signal={:?} generation={} received_at_ms={}",
                    gesture.gesture_id,
                    gesture.source,
                    gesture.signal,
                    gesture.registration_generation,
                    gesture.received_at_ms
                ),
            });
        }
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

        if action == EngineAction::EditSelection {
            if self.state == EngineState::Listening {
                return if self.selected_edit.is_some() {
                    self.try_stop_fire_and_forget(gesture.as_ref(), state_before)
                } else {
                    DispatchResult::Rejected {
                        reason: "Finish the current dictation before editing selected text.".into(),
                    }
                };
            }
            if !self.can_start_from() {
                return DispatchResult::Rejected {
                    reason: "Finish the current operation first.".into(),
                };
            }
            match crate::services::selected_text::SelectedText::capture() {
                Ok(selection) => self.selected_edit = Some(selection),
                Err(error) => {
                    self.emit_phase(DictationPhase::Error, None, error.to_string(), None, None);
                    return DispatchResult::Rejected {
                        reason: error.to_string(),
                    };
                }
            }
            let result = self.enter_listening();
            if !matches!(result, DispatchResult::Accepted) {
                self.selected_edit = None;
            } else if let Some(settings) = self.active_settings.clone() {
                let app = self.app.clone();
                let cancel = self.cancel_requested.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = polish::prepare_selection_editor(&app, &settings, &cancel);
                });
            }
            return result;
        }
        let plan = plan_action(self.state, self.load_mode(), action);
        if matches!(plan, ActionPlan::Start) {
            self.selected_edit = None;
        }
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
        &mut self,
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
        self.pending_diagnostics.push(RuntimeEvent {
            created_at: Utc::now(),
            kind: "shortcut-ack".into(),
            message: format!(
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
        });
    }

    fn enter_listening(&mut self) -> DispatchResult {
        self.cancel_requested.store(false, Ordering::Release);
        self.delivery_boundary.reset();
        match self.begin_listening() {
            Ok(started) => {
                self.state = EngineState::Listening;
                self.active_recording = Some(started.clone());
                self.settle_deadline = None;
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
        if self.state != EngineState::Listening {
            self.selected_edit = None;
        }
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
        self.cancel_requested.store(false, Ordering::Release);
        self.delivery_boundary.reset();
        let started = self.begin_listening().map_err(|e| e)?;
        self.state = EngineState::Listening;
        self.active_recording = Some(started.clone());
        self.settle_deadline = None;
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

    fn processing_cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::Acquire)
    }

    fn clear_processing_cancel(&self) {
        self.cancel_requested.store(false, Ordering::Release);
    }

    fn cancellation_message(session_id: Option<&str>) -> String {
        match session_id {
            Some(session_id) => format!(
                "Dictation cancelled. Captured audio and available text were saved to History ({session_id})."
            ),
            None => "Dictation cancelled. Captured audio and available text were saved to History."
                .to_string(),
        }
    }

    fn fail_pipeline(&mut self, error: String) {
        self.state = EngineState::Error;
        self.active_recording = None;
        self.settle_from_terminal();
        self.emit_phase(DictationPhase::Error, None, error, None, None);
    }

    fn cancel_any(&mut self) -> Result<(), String> {
        self.active_settings = None;
        self.active_cleanup = None;
        self.selected_edit = None;
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
                self.cancel_requested.store(false, Ordering::Release);
                Ok(())
            }
            EngineState::MicCheck => {
                let state = self.app.state::<AppState>();
                state.end_level_stream();
                let _ = state.recorder.cancel();
                self.state = EngineState::Idle;
                self.cancel_requested.store(false, Ordering::Release);
                Ok(())
            }
            // The caller sets the independent cancellation signal before this
            // command reaches the worker. If processing is still active, the
            // signal will be consumed by the pipeline and no late injection is
            // possible; this arm is only reached after the pipeline has
            // yielded back to the command loop.
            EngineState::Processing if self.delivery_boundary.is_committed() => {
                Err("dictation delivery already started".to_string())
            }
            EngineState::Processing => Ok(()),
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

    fn begin_listening(&mut self) -> Result<RecordingStarted, String> {
        let state = self.app.state::<AppState>();
        let (settings, dictionary, snippets) = {
            let database = state.database.lock();
            (
                database.load_settings().map_err(|e| e.to_string())?,
                database.list_dictionary().map_err(|e| e.to_string())?,
                database.list_snippets().map_err(|e| e.to_string())?,
            )
        };
        let target = injection::capture_foreground_target();
        state.set_last_target_window(target.as_ref().map(|target| target.hwnd));
        let settings = crate::services::application_profiles::resolve(
            &settings,
            target
                .as_ref()
                .and_then(|target| target.process_name.as_deref()),
        );
        self.active_settings = Some(settings.clone());
        let prompt = dictionary
            .iter()
            .filter(|entry| entry.enabled)
            .flat_map(|entry| [entry.phrase.as_str(), entry.replacement.as_str()])
            .chain(
                snippets
                    .iter()
                    .filter(|snippet| snippet.enabled)
                    .map(|snippet| snippet.trigger.as_str()),
            )
            .collect::<Vec<_>>()
            .join(", ");
        let streaming = state.streaming_asr().map(|host| recorder::StreamingStart {
            host,
            prompt,
            profile: TranscriptionProfile::Balanced,
        });
        let started = state
            .recorder
            .start_dictation(settings.microphone_name, streaming)
            .map_err(|e| e.to_string())?;
        let context = DictationTextContext {
            cleanup_enabled: settings.cleanup_enabled,
            dictionary,
            snippets,
        };
        self.active_cleanup = Some(context.clone());
        state.live_paste.begin_session(started.id.clone(), context);
        sound_check::start_level_events(&self.app);
        Ok(started)
    }

    fn run_pipeline_from_listening(
        &mut self,
        stop_ack: Option<(&ShortcutGesture, EngineState)>,
    ) -> Result<DictationResult, String> {
        let submit_started = self.request_started;
        let dispatch_ms = submit_started.elapsed().as_millis() as u64;
        let mut timer = StageTimer::from_request(submit_started);
        let recording = self.active_recording.clone();
        self.state = EngineState::Processing;

        // Snapshot live text for the durable History checkpoint; it remains display-only until a final transcript is accepted.
        let stop_outcome = {
            let state = self.app.state::<AppState>();
            let live_snapshot = state.live_paste.take_paste_ready();
            state.end_level_stream();
            let capture_started = Instant::now();
            match state.recorder.stop() {
                Ok(captured) => {
                    let capture_stop_ms = capture_started.elapsed().as_millis() as u64;
                    Ok((live_snapshot, captured, capture_stop_ms))
                }
                Err(error) => {
                    state.live_paste.clear();
                    Err(error.to_string())
                }
            }
        };
        let (live_snapshot, mut captured, capture_stop_ms) = match stop_outcome {
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

        // Snapshot sticky writer flags before finalize_capture consumes their
        // atomics. They remain part of the acceptance decision even after the
        // durable checkpoint has joined the recorder worker.
        let delivery_failed_before_checkpoint = captured.streaming_delivery_failed();
        let write_failed_before_checkpoint = captured.streaming_write_failed();
        let checkpoint_started = Instant::now();
        let capture_finalized = recorder::finalize_capture(&mut captured);
        // The recorder has enqueued every frame. Its Stop barrier can now
        // finalize ASR while the mandatory durable History checkpoint is saved.
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
        if let Err(error) = capture_finalized
            .and_then(|_| self.checkpoint_capture(&mut captured, live_snapshot.as_ref()))
        {
            if let Some(host) = captured.streaming_host.take() {
                host.cancel_session(&captured.id);
            }
            let message = format!(
                "Could not finish saving this dictation: {error}. Captured files were retained."
            );
            self.fail_pipeline(message.clone());
            return Err(message);
        }

        let checkpoint_ms = checkpoint_started.elapsed().as_millis() as u64;

        // The checkpoint is the recovery boundary. If a cancellation arrived
        // while the recorder was being detached, keep the durable source and
        // stop before any transcription or injection work begins.
        if self.processing_cancelled() {
            let session_id = captured.id.clone();
            if let Some(host) = captured.streaming_host.take() {
                host.cancel_session(&session_id);
            }
            self.app.state::<AppState>().live_paste.clear();
            let message = Self::cancellation_message(Some(&session_id));
            self.clear_processing_cancel();
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
            return Err(message);
        }

        // Live hypotheses remain display-only. Release waits for a complete,
        // drop-free streaming Final or decodes the durable WAV in batch.

        // Finalization path: decode the durable source, then paste.
        if self.processing_cancelled() {
            let session_id = captured.id.clone();
            if let Some(host) = captured.streaming_host.take() {
                host.cancel_session(&session_id);
            }
            self.app.state::<AppState>().live_paste.clear();
            let message = Self::cancellation_message(Some(&session_id));
            self.clear_processing_cancel();
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
            return Err(message);
        }
        if write_failed_before_checkpoint || captured.streaming_write_failed() {
            let session_id = captured.id.clone();
            if let Some(host) = captured.streaming_host.take() {
                host.cancel_session(&session_id);
            }
            self.app.state::<AppState>().live_paste.clear();
            let message = format!(
                "Audio capture was incomplete; transcription was not attempted. Captured source was retained in History ({session_id})."
            );
            self.state = EngineState::Error;
            self.active_recording = None;
            self.settle_from_terminal();
            self.clear_processing_cancel();
            self.emit_phase(
                DictationPhase::Error,
                recording,
                message.clone(),
                None,
                None,
            );
            return Err(message);
        }
        self.emit_phase(
            DictationPhase::Finalizing,
            None,
            "Transcribing locally…",
            None,
            None,
        );
        self.app.state::<AppState>().live_paste.clear();

        let prepare_started = Instant::now();
        let analysis =
            recorder::analyze_for_dictation(&captured).map_err(|error| error.to_string())?;
        if let Some(warning) = recorder::dictation_quality_warning(&analysis) {
            metrics::emit_runtime(
                &self.app,
                "audio-quality-warning",
                format!(
                    "session={} duration={}ms warning={warning}",
                    captured.id, captured.duration_ms
                ),
            );
        }

        let prepare_ms = prepare_started.elapsed().as_millis() as u64;
        let snapshot_started = Instant::now();
        let state = self.app.state::<AppState>();
        let settings = self
            .active_settings
            .take()
            .ok_or("Capture settings are unavailable; the recording has been retained")?;
        let context = self
            .active_cleanup
            .take()
            .ok_or("Capture text context is unavailable; the recording has been retained")?;
        let snapshot_ms = snapshot_started.elapsed().as_millis() as u64;
        let submit_prefix_ms = submit_started.elapsed().as_millis() as u64;
        let selected_edit = self.selected_edit.take();
        let editing = selected_edit.is_some();

        let last_hwnd = state.last_target_window();
        let app = self.app.clone();
        let cancel_requested = self.cancel_requested.clone();
        let delivery_boundary = self.delivery_boundary.clone();
        drop(state);

        let pipeline =
            (|| -> Result<(DictationResult, StageMetrics, StreamingMetrics), anyhow::Error> {
                timer.mark_capture_stop(capture_stop_ms);

                let streaming_host = captured.streaming_host.take();
                let streaming_requested = streaming_host.is_some();
                let streaming_frames_dropped = captured.streaming_frames_dropped;
                let write_started = Instant::now();
                let finished = recorder::finish_recording(captured)?;
                timer.mark_write(checkpoint_ms + write_started.elapsed().as_millis() as u64);
                if cancel_requested.load(Ordering::Acquire) {
                    anyhow::bail!("dictation cancelled");
                }

                let mut first_partial_ms = None;
                let mut processed_during_recording_ms = 0;
                let mut tail_audio_ms = finished.duration_ms;
                let mut max_backlog_ms = 0;
                let mut sidecar_frames_dropped = 0;
                let mut fallback_reason = None;
                let mut native_timings = None;
                let asr_started = Instant::now();
                let transcribe_batch = || -> Result<transcriber::Transcription, anyhow::Error> {
                    if cancel_requested.load(Ordering::Acquire) {
                        anyhow::bail!("dictation cancelled");
                    }
                    self.emit_phase(
                        DictationPhase::Finalizing,
                        self.active_recording.clone(),
                        "Recovering saved audio. Cancel leaves the recording in History.",
                        None,
                        None,
                    );
                    metrics::emit_runtime(
                        &app,
                        "asr-replay-start",
                        format!("session={} audio_ms={}", finished.id, finished.duration_ms),
                    );
                    let replay_started = Instant::now();
                    let transcription = transcriber::transcribe_cancellable(
                        &app,
                        &settings,
                        &finished.path,
                        cancel_requested.as_ref(),
                    )?;
                    metrics::emit_runtime(
                        &app,
                        "asr-replay-complete",
                        format!(
                            "session={} elapsed_ms={}",
                            finished.id,
                            replay_started.elapsed().as_millis()
                        ),
                    );
                    if cancel_requested.load(Ordering::Acquire) {
                        anyhow::bail!("dictation cancelled");
                    }
                    Ok(transcription)
                };
                let transcription = if let Some(host) = streaming_host {
                    first_partial_ms = host.first_partial_ms();
                    if !stop_signaled
                        || streaming_drop_requires_fallback(streaming_frames_dropped)
                        || delivery_failed_before_checkpoint
                        || write_failed_before_checkpoint
                    {
                        fallback_reason = Some(if !stop_signaled {
                            "streaming host did not accept the stop signal".to_string()
                        } else if delivery_failed_before_checkpoint {
                            "streaming audio delivery failed before finalization".to_string()
                        } else if write_failed_before_checkpoint {
                            "durable capture writer failed before finalization".to_string()
                        } else {
                            format!(
                                "{streaming_frames_dropped} streaming audio frames were dropped"
                            )
                        });
                        host.cancel_session(&finished.id);
                        transcribe_batch()?
                    } else {
                        if cancel_requested.load(Ordering::Acquire) {
                            host.cancel_session(&finished.id);
                            anyhow::bail!("dictation cancelled");
                        }
                        // Prefer a complete, drop-free streaming Final. A final
                        // carrying any sidecar loss is suspect even if its text
                        // is non-empty; batch decode owns the durable source.
                        match host.await_final_cancellable(
                            &finished.id,
                            Duration::from_secs(30),
                            cancel_requested.as_ref(),
                        ) {
                            Ok(finalized)
                                if streaming_final_is_releaseable(
                                    &finalized.text,
                                    finalized.audio_frames_dropped,
                                ) =>
                            {
                                transcriber::Transcription {
                                    text: {
                                        processed_during_recording_ms =
                                            finalized.processed_during_recording_ms;
                                        tail_audio_ms = finalized.tail_audio_ms;
                                        max_backlog_ms = finalized.max_backlog_ms;
                                        sidecar_frames_dropped = finalized.audio_frames_dropped;
                                        native_timings =
                                            Some((finalized.queue_wait_ms, finalized.decode_ms));
                                        finalized.text
                                    },
                                    backend: match host.backend() {
                                        AsrBackend::Vulkan => metrics::ASR_BACKEND_VULKAN,
                                        _ => metrics::ASR_BACKEND_STREAMING_CPU,
                                    },
                                }
                            }
                            Ok(finalized) if finalized.audio_frames_dropped > 0 => {
                                sidecar_frames_dropped = finalized.audio_frames_dropped;
                                fallback_reason = Some(format!(
                                    "{sidecar_frames_dropped} streaming audio frames were dropped"
                                ));
                                metrics::emit_runtime(
                                    &app,
                                    "streaming-asr-fallback",
                                    format!(
                                        "session={} streaming final reported {} dropped frames; using batch fallback",
                                        finished.id, sidecar_frames_dropped
                                    ),
                                );
                                host.cancel_session(&finished.id);
                                transcribe_batch()?
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
                                transcribe_batch()?
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
                                transcribe_batch()?
                            }
                        }
                    }
                } else {
                    transcribe_batch()?
                };
                if cancel_requested.load(Ordering::Acquire) {
                    anyhow::bail!("dictation cancelled");
                }
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
                if raw_text.trim().is_empty() {
                    anyhow::bail!("No final transcript was returned");
                }
                if cancel_requested.load(Ordering::Acquire) {
                    anyhow::bail!("dictation cancelled");
                }

                let cleanup_started = Instant::now();
                let mut cleaned_text = if editing {
                    raw_text.clone()
                } else {
                    context.prepare_text(&raw_text)
                };
                timer.mark_cleanup(cleanup_started.elapsed().as_millis() as u64);

                let mut session = TranscriptSession {
                    id: finished.id.clone(),
                    raw_text: raw_text.clone(),
                    word_count: cleaned_text.split_whitespace().count(),
                    cleaned_text: cleaned_text.clone(),
                    polished_text: None,
                    prefer_polished: true,
                    audio_path: finished.path.to_string_lossy().to_string(),
                    duration_ms: finished.duration_ms,
                    injected: false,
                    source_application: None,
                    created_at: Utc::now(),
                };
                app.state::<AppState>()
                    .database
                    .lock()
                    .insert_session(&session)?;

                if let Some(selection) = selected_edit.as_ref() {
                    cleaned_text = crate::services::voice_edit::rewrite(
                        &app,
                        &settings,
                        &selection.text,
                        &raw_text,
                        cancel_requested.as_ref(),
                    )?;
                    session.cleaned_text = cleaned_text.clone();
                    session.word_count = cleaned_text.split_whitespace().count();
                    app.state::<AppState>()
                        .database
                        .lock()
                        .insert_session(&session)?;
                }

                // Remote OpenAI-compatible polish stays copy-only unless the user
                // confirms paste-again, so only local providers can change what
                // release pastes.
                let trusted_auto_paste_polish = matches!(
                    settings.polish_provider,
                    crate::models::PolishProvider::Bundled | crate::models::PolishProvider::Ollama
                );
                // Paste is never blocked by work it does not use: only wait on
                // polish when its output is the text being injected. Copy-only
                // providers polish after paste, for history.
                let mut polished_text: Option<String> = None;
                let paste_text = if trusted_auto_paste_polish && !editing {
                    match dictation_polish::run_auto_polish(
                        &app, &settings, &cleaned_text, &finished.id, cancel_requested.as_ref(),
                    ) {
                        Some(text) => {
                            polished_text = Some(text.clone());
                            text
                        }
                        None => cleaned_text.clone(),
                    }
                } else {
                    cleaned_text.clone()
                };

                if cancel_requested.load(Ordering::Acquire) {
                    anyhow::bail!("dictation cancelled");
                }

                if !delivery_boundary.begin_delivery(cancel_requested.as_ref()) {
                    anyhow::bail!("dictation cancelled");
                }
                let (injection_result, paste_ms) = if let Some(selection) =
                    selected_edit.as_ref().filter(|_| settings.auto_inject)
                {
                    let started = Instant::now();
                    let mut result = injection::inject_text_checked(
                        &paste_text,
                        settings.restore_clipboard,
                        None,
                        || selection.validate(),
                    )?;
                    result.target_process_name = selection.target.process_name.clone();
                    if result.injected {
                        result.message =
                            "Selected text replaced. Undo in the target app to restore it.".into();
                    }
                    (Some(result), started.elapsed().as_millis() as u64)
                } else {
                    inject_transcript(
                        &app,
                        &finished.id,
                        &paste_text,
                        settings.auto_inject,
                        settings.restore_clipboard,
                        last_hwnd,
                    )?
                };
                timer.mark_inject(paste_ms);

                // Stop the stage clock at injection, so
                // deferred polish cannot inflate the reported release→paste time.
                let stage_metrics = timer.finish(finished.id.clone(), finished.duration_ms);
                metrics::emit_stage_metrics(&app, &stage_metrics);
                metrics::emit_runtime(
                    &app,
                    "submit-path",
                    format!(
                        "timing_version=2 session={} dispatch_ms={} prefix_ms={} checkpoint_ms={} prepare_ms={} snapshot_ms={} request_to_insertion_ms={} injected={} {}",
                        finished.id,
                        dispatch_ms,
                        submit_prefix_ms,
                        checkpoint_ms,
                        prepare_ms,
                        snapshot_ms,
                        stage_metrics.total_ms,
                        injection_result
                            .as_ref()
                            .is_some_and(|result| result.injected),
                        native_timings
                            .map(|(queue, decode)| format!(
                                "native_queue_ms={queue} native_decode_ms={decode}"
                            ))
                            .unwrap_or_else(|| "native_timing=unavailable".to_string())
                    ),
                );

                session.word_count = paste_text.split_whitespace().count();
                session.injected = injection_result
                    .as_ref()
                    .is_some_and(|result| result.injected);
                session.source_application = injection_result
                    .as_ref()
                    .and_then(|result| result.target_process_name.clone());

                session.polished_text = polished_text;

                let backend = match transcription.backend {
                    metrics::ASR_BACKEND_VULKAN => AsrBackend::Vulkan,
                    metrics::ASR_BACKEND_STREAMING_CPU => AsrBackend::Cpu,
                    metrics::ASR_BACKEND_HOST => AsrBackend::Host,
                    _ => AsrBackend::Cli,
                };
                let streaming_metrics = StreamingMetrics {
                    session_id: finished.id.clone(),
                    backend,
                    model_id: settings.active_model_id.clone(),
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
                let metrics = stage_metrics;
                Ok((
                    DictationResult {
                        session,
                        injection: injection_result,
                    },
                    metrics,
                    streaming_metrics,
                ))
            })();

        let result = self.finish_pipeline_result(recording, pipeline)?;
        if settings.auto_polish && !editing && matches!(settings.polish_provider, crate::models::PolishProvider::OpenaiCompatible) {
            let queued = self.history_polish.as_ref()
                .is_some_and(|worker| worker.submit(settings, result.session.clone()));
            if !queued {
                metrics::emit_runtime(&self.app, "polish-skipped", "Optional editor busy; delivered text remains saved.".to_string());
            }
        }
        Ok(result)
    }

    /// Checkpoint live text and complete audio in existing local History before finalization.
    fn checkpoint_capture(
        &self,
        captured: &mut recorder::CapturedRecording,
        live_snapshot: Option<&crate::services::live_paste::LivePasteSnapshot>,
    ) -> Result<()> {
        recorder::persist_capture_audio(captured)?;
        let raw_text = live_snapshot
            .map(|value| value.raw_text.clone())
            .unwrap_or_default();
        let cleaned_text = live_snapshot
            .map(|value| value.paste_text.clone())
            .unwrap_or_default();
        let session = TranscriptSession {
            id: captured.id.clone(),
            word_count: cleaned_text.split_whitespace().count(),
            raw_text,
            cleaned_text,
            polished_text: None,
            prefer_polished: true,
            audio_path: captured.path.to_string_lossy().to_string(),
            duration_ms: captured.duration_ms,
            injected: false,
            source_application: None,
            created_at: Utc::now(),
        };
        self.app
            .state::<AppState>()
            .database
            .lock()
            .insert_session(&session)?;
        Ok(())
    }

    fn finish_pipeline_result(
        &mut self,
        recording: Option<RecordingStarted>,
        pipeline: Result<(DictationResult, StageMetrics, StreamingMetrics), anyhow::Error>,
    ) -> Result<DictationResult, String> {
        let state = self.app.state::<AppState>();
        if state.streaming_asr().is_some_and(|host| !host.is_alive()) {
            state.shutdown_streaming_asr();
            crate::start_preferred_asr(&self.app);
        }
        if pipeline.is_err() {
            metrics::emit_runtime(
                &self.app,
                "dictation-failed",
                format!(
                    "session={} outcome=failed_or_cancelled; audio and available text retained",
                    recording
                        .as_ref()
                        .map_or("unknown", |recording| recording.id.as_str())
                ),
            );
        }
        match pipeline {
            Ok((result, metrics, streaming_metrics)) => {
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
                let (phase, engine_state) = if injected {
                    (DictationPhase::Pasted, EngineState::Pasted)
                } else if result.injection.is_some() {
                    // Paste soft-failed but clipboard has text
                    (DictationPhase::Error, EngineState::Error)
                } else {
                    (DictationPhase::Saved, EngineState::Pasted)
                };
                let state = self.app.state::<AppState>();
                if let Err(error) = state.database.lock().insert_session(&result.session) {
                    metrics::emit_runtime(&self.app, "session-save-error", error.to_string());
                }
                state.live_paste.clear();
                drop(state);
                // History must contain the completed text before the UI refreshes it.
                self.emit_phase(
                    phase,
                    recording,
                    message,
                    Some(result.clone()),
                    Some(metrics.clone()),
                );
                metrics::emit_streaming_metrics(&self.app, &streaming_metrics);

                self.state = engine_state;
                self.active_recording = None;
                self.settle_from_terminal();
                self.clear_processing_cancel();
                Ok(result)
            }
            Err(error) => {
                let message = if error.to_string() == "dictation cancelled"
                    || self.processing_cancelled()
                {
                    Self::cancellation_message(recording.as_ref().map(|value| value.id.as_str()))
                } else {
                    format!("{error}. Captured audio and available text are saved in History.")
                };
                self.state = EngineState::Error;
                self.active_recording = None;
                self.settle_from_terminal();
                self.clear_processing_cancel();
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
        let event = self.app.state::<AppState>().dictation_snapshot.lock().record(NativeDictationEvent {
            generation: 0,
            recording,
            phase,
            message: message.into(),
            result,
            metrics,
        });
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
    #[test]
    fn recording_snapshot_retains_failure_and_fences_stale_dismissal() {
        use crate::models::{DictationPhase, NativeDictationEvent};
        use crate::services::dictation_state::DictationState;
        let mut state = DictationState::default();
        assert!(state.snapshot().is_none());
        let event = |phase| NativeDictationEvent { generation: 0, phase, recording: None,
            message: "Saved recording".into(), result: None, metrics: None };
        let failed = state.record(event(DictationPhase::Error));
        let idle = state.record(event(DictationPhase::Idle));
        assert!(matches!(idle.phase, DictationPhase::Error));
        assert!(idle.generation > failed.generation);
        assert!(state.dismiss(failed.generation).is_none());
        let dismissed = state.dismiss(idle.generation).unwrap();
        assert!(matches!(state.snapshot().unwrap().phase, DictationPhase::Idle));
        let listening = state.record(event(DictationPhase::Listening));
        assert!(listening.generation > dismissed.generation);
        assert!(state.dismiss(idle.generation).is_none());
        assert!(matches!(state.snapshot().unwrap().phase, DictationPhase::Listening));
    }
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
            ActionPlan::Cancel
        );
        assert_eq!(
            plan_action(EngineState::Idle, PTT, EngineAction::Cancel),
            ActionPlan::Reject("no active recording to cancel")
        );
    }

    /// Any reported sidecar loss forces durable batch fallback.
    #[test]
    fn optional_history_worker_is_bounded_and_shutdown_cancels_queued_work() {
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let worker = HistoryPolishWorker::start(move |_, _, cancelled| {
            entered.send(()).unwrap();
            wait.recv().unwrap();
            finished.send(cancelled.load(Ordering::Acquire)).unwrap();
        }).unwrap();
        let session = TranscriptSession {
            id: "already-delivered".into(), raw_text: "hello".into(), cleaned_text: "Hello.".into(),
            polished_text: None, prefer_polished: true, audio_path: "retained.wav".into(),
            duration_ms: 1000, word_count: 1, injected: true,
            source_application: None, created_at: Utc::now(),
        };
        assert!(worker.submit(crate::models::AppSettings::default(), session.clone()));
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(worker.submit(crate::models::AppSettings::default(), session.clone()));
        assert!(!worker.submit(crate::models::AppSettings::default(), session));
        drop(worker);
        release.send(()).unwrap();
        assert!(done.recv_timeout(Duration::from_secs(2)).unwrap());
        assert!(started.recv_timeout(Duration::from_secs(2)).is_err());
    }

    #[test]
    fn any_streaming_drop_requires_fallback() {
        assert!(!streaming_drop_requires_fallback(0));
        assert!(streaming_drop_requires_fallback(1));
    }

    #[test]
    fn streaming_final_requires_text_and_zero_drops() {
        assert!(!streaming_final_is_releaseable("", 0));
        assert!(!streaming_final_is_releaseable("complete final", 1));
        assert!(streaming_final_is_releaseable("complete final", 0));
    }

    #[test]
    fn cancellation_and_delivery_have_one_linearization_point() {
        let boundary = DeliveryBoundary::new();
        let cancel_requested = std::sync::atomic::AtomicBool::new(false);

        assert!(boundary.begin_delivery(&cancel_requested));
        assert_eq!(
            boundary.request_cancel(&cancel_requested),
            Err("dictation delivery already started")
        );
        assert!(!cancel_requested.load(Ordering::Acquire));

        boundary.reset();
        assert!(boundary.request_cancel(&cancel_requested).is_ok());
        assert!(!boundary.begin_delivery(&cancel_requested));
    }

    /// The worker must not block indefinitely while a settle is pending, or the
    /// overlay stays stuck on the terminal phase until the next user action.
    #[test]
    fn settle_and_submit_timing_use_monotonic_deadlines() {
        let now = Instant::now();
        let measured = StageTimer::from_request(now - Duration::from_millis(75))
            .finish("queued-request".into(), 1_000);
        assert!(
            measured.total_ms >= 75,
            "dispatch wait belongs in submit time"
        );
        assert_eq!(measured.asr_ms, 0);
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
    #[test]
    fn application_profiles_are_exact_and_frozen_for_the_session() {
        use crate::models::{AppSettings, ApplicationProfile, PolishStyle};
        let mut settings = AppSettings::default();
        settings.custom_instructions = "Global guidance".into();
        settings.application_profiles = vec![ApplicationProfile {
            application: "notepad.exe".into(),
            cleanup_enabled: Some(false),
            auto_polish: Some(false),
            polish_style: Some(PolishStyle::Formal),
            instructions: "Keep release names".into(),
        }];
        crate::services::application_profiles::validate(&settings).unwrap();
        let frozen = crate::services::application_profiles::resolve(&settings, Some("Notepad"));
        assert!(!frozen.cleanup_enabled);
        assert_eq!(frozen.polish_style, PolishStyle::Formal);
        assert_eq!(
            frozen.custom_instructions,
            "Global guidance\nKeep release names"
        );
        assert!(
            crate::services::application_profiles::resolve(&settings, Some("notepad2.exe"))
                .cleanup_enabled
        );
        settings.application_profiles[0].cleanup_enabled = Some(true);
        assert!(!frozen.cleanup_enabled);
        settings.application_profiles.push(ApplicationProfile {
            application: "NOTEPAD".into(),
            ..Default::default()
        });
        assert!(crate::services::application_profiles::validate(&settings).is_err());
    }
}
