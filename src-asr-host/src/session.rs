//! Priority-aware worker for the speech sidecar.
//!
//! Audio is accepted through a bounded queue. Control commands have their own
//! queue and are drained before every audio frame and before every native
//! update, so Stop/Cancel never wait behind an audio backlog. The native decoder has no
//! dynamic cancel entry point; the owning client kills this process when a
//! native call must be interrupted.

use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use atmospeak_asr_protocol::{
    AsrBackend, AsrCapabilities, AsrCommand, AsrEvent, PROTOCOL_VERSION, StreamingMetrics,
};

use crate::{decoder::SpeechDecoder, wire};

const AUDIO_DRAIN_BATCH: usize = 25;
const IDLE_WAKE: Duration = Duration::from_millis(100);

pub struct ControlMsg {
    pub command: AsrCommand,
    pub received_at: Instant,
}

pub struct AudioFrameMsg {
    pub session_id: String,
    pub sequence: u64,
    pub received_at: Instant,
    pub pcm_s16le: Vec<u8>,
}

struct Model {
    engine: SpeechDecoder,
    backend: AsrBackend,
}

struct Session {
    id: String,
    total_samples_received: u64,
    unprocessed_samples: usize,
    sequence: Option<u64>,
    expected_last_sequence: Option<u64>,
    revision: u64,
    started: Instant,
    stop_requested_at: Option<Instant>,
    first_partial_ms: Option<u64>,
    max_backlog_ms: u64,
    audio_frames_dropped: u64,
}

impl Session {
    fn new(id: String) -> Self {
        Self {
            id,
            total_samples_received: 0,
            unprocessed_samples: 0,
            sequence: None,
            expected_last_sequence: None,
            revision: 0,
            started: Instant::now(),
            stop_requested_at: None,
            first_partial_ms: None,
            max_backlog_ms: 0,
            audio_frames_dropped: 0,
        }
    }
}

pub fn run_worker(
    control_rx: Receiver<ControlMsg>,
    audio_rx: Receiver<AudioFrameMsg>,
    abort: Arc<AtomicBool>,
    cancel_epoch: Arc<AtomicU64>,
    active_session_id: Arc<Mutex<Option<String>>>,
    output: &mut impl Write,
) -> Result<()> {
    let mut model = None;
    let mut session = None;
    let mut stop_requested = false;
    let mut stop_cancel_epoch = cancel_epoch.load(Ordering::Acquire);

    loop {
        if drain_controls(
            &control_rx,
            &mut model,
            &mut session,
            &mut stop_requested,
            &abort,
            &cancel_epoch,
            &mut stop_cancel_epoch,
            &active_session_id,
            output,
        )? {
            return Ok(());
        }

        let mut drained = 0;
        while drained < AUDIO_DRAIN_BATCH {
            if drain_controls(
                &control_rx,
                &mut model,
                &mut session,
                &mut stop_requested,
                &abort,
                &cancel_epoch,
                &mut stop_cancel_epoch,
                &active_session_id,
                output,
            )? {
                return Ok(());
            }
            match audio_rx.try_recv() {
                Ok(frame) => {
                    // The audio and control queues are intentionally separate.
                    // Receiving a frame proves the sender already enqueued
                    // its preceding Start/Cancel/Stop control, so reconcile
                    // controls again before interpreting the frame.
                    if drain_controls(
                        &control_rx,
                        &mut model,
                        &mut session,
                        &mut stop_requested,
                        &abort,
                        &cancel_epoch,
                        &mut stop_cancel_epoch,
                        &active_session_id,
                        output,
                    )? {
                        return Ok(());
                    }
                    append_frame(
                        &mut model,
                        &mut session,
                        frame,
                        stop_requested,
                        &abort,
                        output,
                    )?;
                    drained += 1;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if session.is_none() {
                        return Ok(());
                    }
                    stop_requested = true;
                    break;
                }
            }
        }

        if stop_requested {
            if drained > 0 {
                continue;
            }
            let Some(active) = session.as_mut() else {
                stop_requested = false;
                abort.store(false, Ordering::Release);
                continue;
            };
            account_tail_loss(active);
            finalize_stopped_session(
                &mut model,
                &mut session,
                &cancel_epoch,
                stop_cancel_epoch,
                &abort,
                &active_session_id,
                output,
            )?;
            stop_requested = false;
            continue;
        }

        if drained > 0 {
            // A queued burst should be consumed without parking between
            // frames. The next pass still drains controls before another
            // dequeue, preserving Stop/Cancel priority.
            continue;
        }

        // The reader unparks this worker after every successful dispatch. The
        // park token closes the race between observing both queues empty and
        // going to sleep, without polling the audio queue every 100 ms.
        std::thread::park_timeout(IDLE_WAKE);
    }
}

fn drain_controls(
    control_rx: &Receiver<ControlMsg>,
    model: &mut Option<Model>,
    session: &mut Option<Session>,
    stop_requested: &mut bool,
    abort: &Arc<AtomicBool>,
    cancel_epoch: &Arc<AtomicU64>,
    stop_cancel_epoch: &mut u64,
    active_session_id: &Arc<Mutex<Option<String>>>,
    output: &mut impl Write,
) -> Result<bool> {
    loop {
        match control_rx.try_recv() {
            Ok(message) => {
                if handle_control(
                    message.command,
                    message.received_at,
                    model,
                    session,
                    stop_requested,
                    abort,
                    cancel_epoch,
                    stop_cancel_epoch,
                    active_session_id,
                    output,
                )? {
                    return Ok(true);
                }
            }
            Err(TryRecvError::Empty) => return Ok(false),
            Err(TryRecvError::Disconnected) => return Ok(false),
        }
    }
}

fn publish_active_session(active_session_id: &Arc<Mutex<Option<String>>>, id: Option<String>) {
    if let Ok(mut guard) = active_session_id.lock() {
        *guard = id;
    }
}

fn handle_control(
    command: AsrCommand,
    received_at: Instant,
    model: &mut Option<Model>,
    session: &mut Option<Session>,
    stop_requested: &mut bool,
    abort: &Arc<AtomicBool>,
    cancel_epoch: &Arc<AtomicU64>,
    stop_cancel_epoch: &mut u64,
    active_session_id: &Arc<Mutex<Option<String>>>,
    output: &mut impl Write,
) -> Result<bool> {
    match command {
        AsrCommand::Hello { protocol_version } => {
            if protocol_version != PROTOCOL_VERSION {
                write_event(
                    output,
                    &AsrEvent::Error {
                        session_id: None,
                        recoverable: false,
                        message: format!(
                            "protocol mismatch: host={PROTOCOL_VERSION}, client={protocol_version}"
                        ),
                    },
                )?;
                bail!("unsupported protocol version")
            }
        }
        AsrCommand::LoadModel {
            model_path,
            backend,
            threads,
        } => match SpeechDecoder::load(std::path::Path::new(&model_path), backend, threads) {
            Ok(engine) => {
                write_event(
                    output,
                    &AsrEvent::Ready {
                        capabilities: AsrCapabilities {
                            protocol_version: PROTOCOL_VERSION,
                            backend,
                            streaming: engine.supports_streaming(),
                            vad: false,
                        },
                    },
                )?;
                *model = Some(Model { engine, backend });
            }
            Err(error) => write_error(output, None, &error.to_string(), true)?,
        },
        AsrCommand::StartSession {
            session_id,
            language,
            initial_prompt: _,
            profile: _profile,
        } => {
            if model.is_none() {
                write_error(output, Some(session_id), "model is not loaded", true)?;
                return Ok(false);
            }
            if let Some(active) = session.as_ref() {
                write_error(
                    output,
                    Some(session_id),
                    &format!("session {} is already active", active.id),
                    true,
                )?;
                return Ok(false);
            }
            let engine = &mut model.as_mut().expect("model checked above").engine;
            if let Err(error) = engine.start(&language) {
                write_error(output, Some(session_id), &error.to_string(), true)?;
                return Ok(false);
            }
            abort.store(false, Ordering::Release);
            *stop_cancel_epoch = cancel_epoch.load(Ordering::Acquire);
            publish_active_session(active_session_id, Some(session_id.clone()));
            *session = Some(Session::new(session_id));
            *stop_requested = false;
        }
        AsrCommand::AudioFrame { .. } => {}
        AsrCommand::StopSession {
            session_id,
            last_sequence,
        } => {
            if let Some(active) = session.as_mut().filter(|active| active.id == session_id) {
                active.expected_last_sequence = last_sequence;
                active.stop_requested_at.get_or_insert(received_at);
                *stop_cancel_epoch = cancel_epoch.load(Ordering::Acquire);
                *stop_requested = true;
                // The native decoder has no abort callback. This flag prevents another
                // partial update; the client owns process termination if a
                // native call must be interrupted.
                abort.store(true, Ordering::Release);
            } else {
                abort.store(false, Ordering::Release);
                write_error(output, Some(session_id), "session is not active", true)?;
            }
        }
        AsrCommand::CancelSession { session_id } => {
            if session
                .as_ref()
                .is_some_and(|active| active.id == session_id)
            {
                *session = None;
                *stop_requested = false;
                publish_active_session(active_session_id, None);
                cancel_epoch.fetch_add(1, Ordering::AcqRel);
                if let Some(model) = model.as_mut() {
                    model.engine.discard_stream();
                }
                abort.store(true, Ordering::Release);
            }
        }
        AsrCommand::Shutdown => return Ok(true),
    }
    Ok(false)
}


fn append_frame(
    model: &mut Option<Model>,
    session: &mut Option<Session>,
    frame: AudioFrameMsg,
    stop_requested: bool,
    interrupted: &AtomicBool,
    output: &mut impl Write,
) -> Result<()> {
    let Some(active) = session
        .as_mut()
        .filter(|active| active.id == frame.session_id)
    else {
        return Ok(());
    };
    let expected = active
        .sequence
        .map_or(0, |sequence| sequence.saturating_add(1));
    if frame.sequence != expected {
        active.audio_frames_dropped = active
            .audio_frames_dropped
            .saturating_add(frame.sequence.saturating_sub(expected));
        if frame.sequence < expected {
            return Ok(());
        }
        write_error(
            output,
            Some(active.id.clone()),
            &format!(
                "audio sequence gap: expected {expected}, received {}",
                frame.sequence
            ),
            true,
        )?;
    }
    active.sequence = Some(frame.sequence);
    let queue_age_ms = frame.received_at.elapsed().as_millis() as u64;
    active.max_backlog_ms = active.max_backlog_ms.max(queue_age_ms);
    let samples = wire::pcm_bytes_to_f32(&frame.pcm_s16le)?;
    active.total_samples_received = active
        .total_samples_received
        .saturating_add(samples.len() as u64);
    active.unprocessed_samples = active.unprocessed_samples.saturating_add(samples.len());
    let Some(model) = model.as_mut() else {
        return Ok(());
    };
    let should_update = model.engine.push(&samples)?;
    // Audio sample time and wall time can drift or start at different moments.
    // Coalesce display updates without changing the native decoder cadence.
    // Finalization still decodes every accepted sample.
    let preview_is_current = queue_age_ms <= 1_000;
    if should_update {
        // Reconcile a newly received Stop/Cancel before another native call.
        if !stop_requested && interrupted.load(Ordering::Acquire) {
            return Ok(());
        }
        if stop_requested || !preview_is_current {
            // Preserve the same decoder steps as saved-WAV replay. Skipping
            // them changes word choices and accumulates a larger final tail.
            // Only the outdated display text is expendable.
            model.engine.advance()?;
            active.unprocessed_samples = 0;
            return Ok(());
        }
        let text = model.engine.update()?;
        active.unprocessed_samples = 0;
        // Submit or Cancel may arrive during the native call. Keep its decoder
        // state, but do not publish another preview ahead of the final result.
        if interrupted.load(Ordering::Acquire) {
            return Ok(());
        }
        if !text.trim().is_empty() {
            active.revision = active.revision.saturating_add(1);
            let elapsed = active.started.elapsed().as_millis() as u64;
            active.first_partial_ms.get_or_insert(elapsed.max(1));
            write_event(
                output,
                &AsrEvent::Partial {
                    session_id: active.id.clone(),
                    revision: active.revision,
                    text,
                    covered_through_ms: wire::samples_to_ms(
                        active.total_samples_received as usize,
                    ),
                },
            )?;
        }
    }
    Ok(())
}

fn account_tail_loss(active: &mut Session) {
    let Some(last_sequence) = active.expected_last_sequence else {
        return;
    };
    let missing = match active.sequence {
        Some(received) => last_sequence.saturating_sub(received),
        None => last_sequence.saturating_add(1),
    };
    active.audio_frames_dropped = active.audio_frames_dropped.saturating_add(missing);
    if active
        .sequence
        .is_none_or(|received| last_sequence > received)
    {
        active.sequence = Some(last_sequence);
    }
}

fn finalize_stopped_session(
    model: &mut Option<Model>,
    session: &mut Option<Session>,
    cancel_epoch: &Arc<AtomicU64>,
    stop_cancel_epoch: u64,
    abort: &Arc<AtomicBool>,
    active_session_id: &Arc<Mutex<Option<String>>>,
    output: &mut impl Write,
) -> Result<()> {
    let Some(active) = session.as_mut() else {
        return Ok(());
    };
    if cancel_epoch.load(Ordering::Acquire) != stop_cancel_epoch {
        discard_session(session, abort, active_session_id);
        return Ok(());
    }
    let finalize_started = Instant::now();
    let tail_audio_ms = wire::samples_to_ms(active.unprocessed_samples);
    let session_id = active.id.clone();
    let stop_ack_ms = active
        .stop_requested_at
        .map(|started| started.elapsed().as_millis() as u64)
        .unwrap_or_default();
    let Some(loaded) = model.as_mut() else {
        discard_session(session, abort, active_session_id);
        return Ok(());
    };
    let text = match loaded.engine.finish() {
        Ok(text) => text,
        Err(error) => {
            write_error(output, Some(session_id), &error.to_string(), true)?;
            discard_session(session, abort, active_session_id);
            return Ok(());
        }
    };
    if cancel_epoch.load(Ordering::Acquire) != stop_cancel_epoch {
        discard_session(session, abort, active_session_id);
        return Ok(());
    }
    let audio_ms = wire::samples_to_ms(active.total_samples_received as usize);
    let processed_during_recording_ms = audio_ms.saturating_sub(tail_audio_ms);
    let metrics = StreamingMetrics {
        session_id: session_id.clone(),
        backend: loaded.backend,
        model_id: loaded.engine.model().id.clone(),
        first_partial_ms: active.first_partial_ms,
        stop_ack_ms,
        finalize_ms: finalize_started.elapsed().as_millis() as u64,
        paste_ms: 0,
        processed_during_recording_ms,
        tail_audio_ms,
        max_backlog_ms: active.max_backlog_ms,
        audio_frames_dropped: active.audio_frames_dropped,
        fallback_reason: None,
    };
    // The client refuses a Final without the matching Metrics event.
    write_event(output, &AsrEvent::Metrics(metrics))?;
    write_event(
        output,
        &AsrEvent::Final {
            session_id,
            text,
            processed_during_recording_ms,
            tail_audio_ms,
        },
    )?;
    *session = None;
    publish_active_session(active_session_id, None);
    abort.store(false, Ordering::Release);
    Ok(())
}

fn discard_session(
    session: &mut Option<Session>,
    abort: &Arc<AtomicBool>,
    active_session_id: &Arc<Mutex<Option<String>>>,
) {
    *session = None;
    publish_active_session(active_session_id, None);
    abort.store(false, Ordering::Release);
}

fn write_event(output: &mut impl Write, event: &AsrEvent) -> Result<()> {
    wire::write_event(output, event)
}

fn write_error(
    output: &mut impl Write,
    session_id: Option<String>,
    message: &str,
    recoverable: bool,
) -> Result<()> {
    wire::write_error(output, session_id, message, recoverable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_protocol_version_is_rejected_before_model_use() {
        let abort = Arc::new(AtomicBool::new(false));
        let active_session_id = Arc::new(Mutex::new(None));
        let mut model = None;
        let mut session = None;
        let mut stop_requested = false;
        let cancel_epoch = Arc::new(AtomicU64::new(0));
        let mut stop_cancel_epoch = 0;
        let mut output = Vec::new();

        let error = handle_control(
            AsrCommand::Hello {
                protocol_version: PROTOCOL_VERSION - 1,
            },
            Instant::now(),
            &mut model,
            &mut session,
            &mut stop_requested,
            &abort,
            &cancel_epoch,
            &mut stop_cancel_epoch,
            &active_session_id,
            &mut output,
        )
        .expect_err("stale host/client must fail the handshake");
        assert!(error.to_string().contains("unsupported protocol version"));

        let event = rmp_serde::from_slice::<AsrEvent>(&output[4..]).expect("error event");
        assert!(matches!(
            event,
            AsrEvent::Error {
                recoverable: false,
                session_id: None,
                ..
            }
        ));
    }

    #[test]
    fn tail_loss_is_accounted_without_a_later_sequence_gap() {
        let mut session = Session::new("s".to_string());
        session.sequence = Some(4);
        session.expected_last_sequence = Some(7);
        account_tail_loss(&mut session);
        assert_eq!(session.audio_frames_dropped, 3);

        // Accounting the same stop barrier twice must not double count.
        account_tail_loss(&mut session);
        assert_eq!(session.audio_frames_dropped, 3);
    }

    #[test]
    fn leading_sequence_gap_and_queue_age_ignore_producer_start_delay() {
        let mut model = None;
        let mut session = Some(Session::new("s".to_string()));
        let now = Instant::now();
        let received_at = now.checked_sub(Duration::from_secs(2)).unwrap_or(now);
        let minimum_queue_age = now.duration_since(received_at).as_millis() as u64;
        session.as_mut().expect("session").started =
            now.checked_sub(Duration::from_secs(3_600)).unwrap_or(now);
        let frame = AudioFrameMsg {
            session_id: "s".to_string(),
            sequence: 3,
            received_at,
            pcm_s16le: vec![0, 0],
        };
        append_frame(
            &mut model,
            &mut session,
            frame,
            false,
            &AtomicBool::new(false),
            &mut Vec::new(),
        )
        .expect("append");
        let session = session.as_ref().expect("session");
        assert_eq!(session.audio_frames_dropped, 3);
        assert!(session.max_backlog_ms >= minimum_queue_age);
        assert!(session.max_backlog_ms <= received_at.elapsed().as_millis() as u64);
    }

    #[test]
    fn stale_audio_from_cancelled_session_is_discarded_after_next_session_starts() {
        let mut model = None;
        let mut session = Some(Session::new("following".to_string()));
        let stale = AudioFrameMsg {
            session_id: "old".to_string(),
            sequence: 0,
            received_at: Instant::now(),
            pcm_s16le: vec![0, 0],
        };
        let mut output = Vec::new();

        append_frame(
            &mut model,
            &mut session,
            stale,
            false,
            &AtomicBool::new(false),
            &mut output,
        )
        .expect("stale audio is discardable");

        assert!(
            output.is_empty(),
            "cancelled audio must not report a false error"
        );
        assert_eq!(
            session
                .as_ref()
                .expect("following session")
                .total_samples_received,
            0
        );
    }

    #[test]
    fn cancellation_epoch_wins_over_stop_abort_reset() {
        let abort = Arc::new(AtomicBool::new(true));
        let cancel_epoch = Arc::new(AtomicU64::new(9));
        let active_session_id = Arc::new(Mutex::new(Some("s".to_string())));
        let mut session = Some(Session::new("s".to_string()));
        let mut output = Vec::new();

        // The stop path captured epoch 8, then Cancel arrived while the
        // worker was between its control drain and finalize entry. A plain
        // abort flag would be cleared by finalize's stop reset; the epoch
        // must discard the session before touching a model.
        finalize_stopped_session(
            &mut None,
            &mut session,
            &cancel_epoch,
            8,
            &abort,
            &active_session_id,
            &mut output,
        )
        .expect("cancellation is a normal terminal outcome");

        assert!(session.is_none());
        assert!(output.is_empty());
        assert!(!abort.load(Ordering::Acquire));
        assert!(active_session_id.lock().expect("active id").is_none());
    }

    #[test]
    fn control_is_drained_before_the_following_audio_frame() {
        let abort = Arc::new(AtomicBool::new(false));
        let cancel_epoch = Arc::new(AtomicU64::new(0));
        let active_session_id = Arc::new(Mutex::new(Some("s".to_string())));
        let mut stop_cancel_epoch = 0;
        let mut stop_requested = false;
        let mut model = None;
        let mut session = Some(Session::new("s".to_string()));
        let (control_tx, control_rx) = std::sync::mpsc::channel();
        let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel(1);
        let received_at = Instant::now()
            .checked_sub(Duration::from_millis(20))
            .expect("earlier stop receipt");
        control_tx
            .send(ControlMsg {
                command: AsrCommand::StopSession {
                    session_id: "s".to_string(),
                    last_sequence: Some(0),
                },
                received_at,
            })
            .expect("stop control");
        audio_tx
            .send(AudioFrameMsg {
                session_id: "s".to_string(),
                sequence: 0,
                received_at: Instant::now(),
                pcm_s16le: vec![0, 0],
            })
            .expect("audio frame");

        let frame = audio_rx.try_recv().expect("queued audio frame");
        let exited = drain_controls(
            &control_rx,
            &mut model,
            &mut session,
            &mut stop_requested,
            &abort,
            &cancel_epoch,
            &mut stop_cancel_epoch,
            &active_session_id,
            &mut Vec::new(),
        )
        .expect("control processing");

        assert!(!exited);
        assert!(stop_requested, "control must be handled before audio");
        assert_eq!(
            session.as_ref().unwrap().stop_requested_at,
            Some(received_at)
        );
        append_frame(
            &mut model,
            &mut session,
            frame,
            stop_requested,
            &abort,
            &mut Vec::new(),
        )
        .expect("audio append");
        assert_eq!(session.as_ref().and_then(|active| active.sequence), Some(0));
    }
}
