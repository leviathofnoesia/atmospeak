//! The inference worker: owns the model and the active session. Audio appends
//! are cheap and never wait on a decode; control commands (stop above all) are
//! handled before any inference work, so hotkey release reconciles only the
//! uncommitted tail instead of draining a backlog of previews.

use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, TryRecvError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, bail};
use atmospeak_asr_protocol::{
    AsrBackend, AsrCapabilities, AsrCommand, AsrEvent, PROTOCOL_VERSION, TranscriptionProfile,
};
use whisper_rs::{
    WhisperContext, WhisperContextParameters, WhisperState, WhisperVadContext,
    WhisperVadContextParams,
};

use crate::inference::{
    self, SAMPLE_RATE, samples_to_ms, vad_window_start, write_error, write_event,
};

const PREVIEW_INTERVAL: Duration = Duration::from_secs(1);
const PREVIEW_SAMPLES: usize = SAMPLE_RATE * 6;
const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE / 4;
/// Mid-utterance commit after this much trailing silence (was 500 ms).
const SILENCE_SAMPLES: usize = SAMPLE_RATE * 3 / 10;
/// Force-commit long uncommitted tails so stop only decodes a short remainder.
const FORCE_SPLIT_SAMPLES: usize = SAMPLE_RATE * 2;
const OVERLAP_SAMPLES: usize = SAMPLE_RATE / 2;
const VAD_CHECK_SAMPLES: usize = SAMPLE_RATE / 10;
const VAD_CADENCE: Duration = Duration::from_millis(100);
const VAD_MODEL_FILENAME: &str = "ggml-silero-v6.2.0.bin";
/// Audio frames appended per worker pass — bounds how long appends can delay a
/// pending control command. Keep this small enough that VAD still runs about
/// every half-second while audio is flowing (~20 ms frames).
const AUDIO_DRAIN_BATCH: usize = 25;

pub struct AudioFrameMsg {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub pcm_s16le: Vec<u8>,
}

struct Model {
    /// Kept so the context outlives `state` (whisper-rs state borrows the ctx).
    #[allow(dead_code)]
    context: WhisperContext,
    /// Reused across commits/previews/finalize — creating a state per decode
    /// was a multi-hundred-ms fixed cost on the release→paste path.
    state: WhisperState,
    backend: AsrBackend,
    threads: i32,
    model_id: String,
    vad: WhisperVadContext,
    /// StopSession sets this so in-flight `whisper_full` aborts promptly.
    abort: Arc<AtomicBool>,
}

struct Session {
    id: String,
    language: String,
    prompt: String,
    profile: TranscriptionProfile,
    audio: Vec<f32>,
    /// Absolute sample position represented by audio[0]. Keeping this lets us
    /// discard committed audio without losing event timestamps.
    audio_base_samples: u64,
    total_samples_received: u64,
    committed: String,
    chunk_start: usize,
    last_vad_check: usize,
    speech_active: bool,
    sequence: Option<u64>,
    revision: u64,
    segment_index: u32,
    last_preview: Instant,
    started: Instant,
    first_partial_ms: Option<u64>,
    max_backlog_ms: u64,
    audio_frames_dropped: u64,
}

impl Session {
    fn new(
        id: String,
        language: String,
        prompt: String,
        profile: TranscriptionProfile,
    ) -> Self {
        Self {
            id,
            language,
            prompt,
            profile,
            audio: Vec::new(),
            audio_base_samples: 0,
            total_samples_received: 0,
            committed: String::new(),
            chunk_start: 0,
            last_vad_check: 0,
            speech_active: false,
            sequence: None,
            revision: 0,
            segment_index: 0,
            last_preview: Instant::now(),
            started: Instant::now(),
            first_partial_ms: None,
            max_backlog_ms: 0,
            audio_frames_dropped: 0,
        }
    }
}

pub fn run_worker(
    control_rx: Receiver<AsrCommand>,
    audio_rx: Receiver<AudioFrameMsg>,
    abort: Arc<AtomicBool>,
    active_session_id: Arc<Mutex<Option<String>>>,
    output: &mut impl Write,
) -> Result<()> {
    let mut model: Option<Model> = None;
    let mut session: Option<Session> = None;
    let mut stop_requested = false;

    loop {
        // 1. Control before inference: StopSession must never queue behind a
        // preview decode.
        match control_rx.try_recv() {
            Ok(command) => {
                if handle_control(
                    command,
                    &mut model,
                    &mut session,
                    &mut stop_requested,
                    &abort,
                    &active_session_id,
                    output,
                )? {
                    return Ok(());
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(()),
        }

        // 2. Audio appends are cheap; keep ingestion caught up so backlog
        // metrics reflect decode slowness, not scheduling slowness.
        let mut drained = 0_usize;
        while drained < AUDIO_DRAIN_BATCH {
            match audio_rx.try_recv() {
                Ok(frame) => {
                    append_frame(&mut session, frame, output)?;
                    drained += 1;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        // 3. Stop fast path: once the tail has arrived, reconcile it without
        // running another preview first.
        if stop_requested {
            // Drain any frames still in the channel before reconciling — a
            // momentarily empty queue is not proof the reader is done if the
            // control command raced ahead of a buffered AudioFrame.
            let mut drained = 0_usize;
            while drained < AUDIO_DRAIN_BATCH {
                match audio_rx.try_recv() {
                    Ok(frame) => {
                        append_frame(&mut session, frame, output)?;
                        drained += 1;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            if drained > 0 {
                continue;
            }
            // Honor CancelSession that raced ahead of finalize so we never
            // emit Final for a session the client already canceled.
            loop {
                match control_rx.try_recv() {
                    Ok(command) => {
                        if handle_control(
                            command,
                            &mut model,
                            &mut session,
                            &mut stop_requested,
                            &abort,
                            &active_session_id,
                            output,
                        )? {
                            return Ok(());
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
            if session.is_none() {
                stop_requested = false;
                abort.store(false, Ordering::Release);
                continue;
            }
            finalize_stopped_session(
                &mut model,
                &mut session,
                &abort,
                &active_session_id,
                output,
            )?;
            stop_requested = false;
            continue;
        }

        // 4. Scheduled inference work.
        if let Some(active) = session.as_ref() {
            if vad_check_due(active) {
                // StopSession may have arrived while we drained audio. Never
                // start another multi-hundred-ms commit that delays the stop
                // fast path — finalize_stopped_session owns the remaining tail.
                match control_rx.try_recv() {
                    Ok(command) => {
                        if handle_control(
                            command,
                            &mut model,
                            &mut session,
                            &mut stop_requested,
                            &abort,
                            &active_session_id,
                            output,
                        )? {
                            return Ok(());
                        }
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
                if stop_requested {
                    continue;
                }
                let mut active = session.take().expect("session checked above");
                run_vad_pass(&mut model, &mut active, output)?;
                session = Some(active);
                continue;
            }
            // Previews are best-effort UI candy: only while fully caught up and
            // never when StopSession work should stay ahead of dock polish.
            if preview_due(active) && active.max_backlog_ms <= 500 {
                match audio_rx.try_recv() {
                    Ok(frame) => {
                        append_frame(&mut session, frame, output)?;
                        continue;
                    }
                    Err(TryRecvError::Empty) => {
                        let mut active = session.take().expect("session checked above");
                        emit_preview(
                            model.as_mut().ok_or_else(|| anyhow!("model is not loaded"))?,
                            &mut active,
                            output,
                        )?;
                        session = Some(active);
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => {}
                }
            } else if preview_due(active) {
                if let Some(active) = session.as_mut() {
                    active.last_preview = Instant::now();
                }
            }
        }

        // 5. Idle: wait for more audio, waking at the VAD cadence.
        match audio_rx.recv_timeout(VAD_CADENCE) {
            Ok(frame) => append_frame(&mut session, frame, output)?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if session.is_none() {
                    return Ok(());
                }
            }
        }
    }
}

fn publish_active_session(active_session_id: &Arc<Mutex<Option<String>>>, id: Option<String>) {
    if let Ok(mut guard) = active_session_id.lock() {
        *guard = id;
    }
}

/// Ok(true) means the worker should exit.
fn handle_control(
    command: AsrCommand,
    model: &mut Option<Model>,
    session: &mut Option<Session>,
    stop_requested: &mut bool,
    abort: &Arc<AtomicBool>,
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
                bail!("unsupported protocol version");
            }
        }
        AsrCommand::LoadModel {
            model_path,
            backend,
            threads,
        } => {
            if let Some(loaded) = load_model(&model_path, backend, threads, abort, output)? {
                *model = Some(loaded);
            }
        }
        AsrCommand::StartSession {
            session_id,
            language,
            initial_prompt,
            profile,
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
            abort.store(false, Ordering::Release);
            publish_active_session(active_session_id, Some(session_id.clone()));
            *session = Some(Session::new(session_id, language, initial_prompt, profile));
            *stop_requested = false;
        }
        AsrCommand::AudioFrame { .. } => {
            // The reader thread routes audio over its own channel; a frame on
            // the control channel indicates a client bug, not a fatal error.
        }
        AsrCommand::StopSession { session_id } => {
            if session
                .as_ref()
                .is_some_and(|active| active.id == session_id)
            {
                *stop_requested = true;
                // Abort any in-flight whisper_full so StopSession is not stuck
                // behind a multi-second force-split commit.
                abort.store(true, Ordering::Release);
            } else {
                // Reader only aborts for the published active id; clear any
                // stale flag if a mismatched stop raced the publish update.
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
                abort.store(true, Ordering::Release);
            } else {
                abort.store(false, Ordering::Release);
            }
        }
        AsrCommand::Shutdown => return Ok(true),
    }
    Ok(false)
}

fn load_model(
    model_path: &str,
    backend: AsrBackend,
    threads: u16,
    abort: &Arc<AtomicBool>,
    output: &mut impl Write,
) -> Result<Option<Model>> {
    if backend == AsrBackend::Vulkan && !cfg!(feature = "vulkan") {
        write_event(
            output,
            &AsrEvent::Error {
                session_id: None,
                recoverable: true,
                message: "this host was built without Vulkan".to_string(),
            },
        )?;
        return Ok(None);
    }
    let mut parameters = WhisperContextParameters::default();
    parameters.use_gpu(backend == AsrBackend::Vulkan);
    parameters.flash_attn(true);
    let context = WhisperContext::new_with_params(model_path, parameters)
        .map_err(|error| anyhow!("failed to load model: {error}"))?;
    let state = context
        .create_state()
        .map_err(|error| anyhow!("failed to create decoder state: {error}"))?;
    let vad_path = std::env::var("ATMOSPEAK_VAD_MODEL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
                .unwrap_or_default()
                .join(VAD_MODEL_FILENAME)
        });
    let mut vad_context_params = WhisperVadContextParams::default();
    vad_context_params.set_n_threads(i32::from(threads.max(1)));
    vad_context_params.set_use_gpu(false);
    let vad = WhisperVadContext::new(&vad_path.to_string_lossy(), vad_context_params)
        .map_err(|error| anyhow!("failed to load VAD model {}: {error}", vad_path.display()))?;
    let model_id = std::path::Path::new(model_path)
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "custom".to_string());
    write_event(
        output,
        &AsrEvent::Ready {
            capabilities: AsrCapabilities {
                protocol_version: PROTOCOL_VERSION,
                backend,
                streaming: true,
                vad: true,
            },
        },
    )?;
    Ok(Some(Model {
        context,
        state,
        backend,
        threads: i32::from(threads.max(1)),
        model_id,
        vad,
        abort: Arc::clone(abort),
    }))
}

fn append_frame(
    session: &mut Option<Session>,
    frame: AudioFrameMsg,
    output: &mut impl Write,
) -> Result<()> {
    let Some(active) = session
        .as_mut()
        .filter(|active| active.id == frame.session_id)
    else {
        write_error(output, Some(frame.session_id), "session is not active", true)?;
        return Ok(());
    };
    if let Some(previous) = active.sequence
        && frame.sequence != previous + 1
    {
        active.audio_frames_dropped += frame.sequence.saturating_sub(previous + 1);
        write_error(
            output,
            Some(active.id.clone()),
            &format!(
                "audio sequence gap: expected {}, received {}",
                previous + 1,
                frame.sequence
            ),
            true,
        )?;
    }
    active.sequence = Some(frame.sequence);
    let backlog_ms = active
        .started
        .elapsed()
        .as_millis()
        .saturating_sub(u128::from(frame.timestamp_ms.saturating_add(20))) as u64;
    active.max_backlog_ms = active.max_backlog_ms.max(backlog_ms);
    let samples = inference::pcm_bytes_to_f32(&frame.pcm_s16le)?;
    active.total_samples_received += samples.len() as u64;
    active.audio.extend_from_slice(&samples);
    Ok(())
}

fn vad_check_due(active: &Session) -> bool {
    active.audio.len().saturating_sub(active.chunk_start) >= SAMPLE_RATE
        && active.audio.len().saturating_sub(active.last_vad_check) >= VAD_CHECK_SAMPLES
}

fn preview_due(active: &Session) -> bool {
    active.audio.len() >= SAMPLE_RATE && active.last_preview.elapsed() >= PREVIEW_INTERVAL
}

fn run_vad_pass(
    model: &mut Option<Model>,
    active: &mut Session,
    output: &mut impl Write,
) -> Result<()> {
    active.last_vad_check = active.audio.len();
    let window_start = vad_window_start(active.chunk_start, active.audio.len());
    let model_mut = model
        .as_mut()
        .ok_or_else(|| anyhow!("model is not loaded"))?;
    let last_speech_end =
        inference::vad_last_speech_end(&mut model_mut.vad, &active.audio[window_start..])?
            .map(|end| window_start + end);
    let speech_active = last_speech_end
        .is_some_and(|end| active.audio.len().saturating_sub(end) < SILENCE_SAMPLES);
    if speech_active != active.speech_active {
        active.speech_active = speech_active;
        write_event(
            output,
            &AsrEvent::SpeechState {
                session_id: active.id.clone(),
                active: speech_active,
            },
        )?;
    }
    let chunk_len = active.audio.len().saturating_sub(active.chunk_start);
    // Prefer silence-boundary commits. Force-split only when the uncommitted
    // tail is long enough that waiting for stop would decode a multi-second
    // chunk; keep the threshold modest so StopSession rarely waits on a
    // multi-second in-flight commit.
    let finalize = chunk_len >= FORCE_SPLIT_SAMPLES
        || last_speech_end.is_some_and(|end| {
            end.saturating_sub(active.chunk_start) >= MIN_SPEECH_SAMPLES
                && active.audio.len().saturating_sub(end) >= SILENCE_SAMPLES
        });
    if finalize {
        finalize_chunk(model_mut, active, output)?;
    }
    Ok(())
}

fn model_slot_mut(model: &mut Option<Model>) -> Result<&mut Model> {
    model.as_mut().ok_or_else(|| anyhow!("model is not loaded"))
}

fn finalize_stopped_session(
    model: &mut Option<Model>,
    session: &mut Option<Session>,
    abort: &Arc<AtomicBool>,
    active_session_id: &Arc<Mutex<Option<String>>>,
    output: &mut impl Write,
) -> Result<()> {
    let Some(active) = session.as_mut() else {
        return Ok(());
    };
    // Keep the active session id published so CancelSession during this decode
    // can still set abort from the reader thread.
    abort.store(false, Ordering::Release);
    let finalize_started = Instant::now();
    let tail_audio_ms = samples_to_ms(active.audio.len().saturating_sub(active.chunk_start));
    // Always decode a meaningful uncommitted tail on stop. Skipping when VAD
    // misses speech (common with short fixture / TTS clips) left `committed`
    // empty and forced a multi-second batch fallback on the critical path.
    // Skip decoding a near-silent stop tail — whisper often invents a stray
    // token on padded silence, and the commit already holds the utterance.
    if active.audio.len() > active.chunk_start + SAMPLE_RATE / 10 {
        let speechy = {
            let model_mut = model_slot_mut(model)?;
            let tail = &active.audio[active.chunk_start..];
            // Scan the full uncommitted tail. A prefix-only window misses speech
            // that arrives after a long silent stretch when VAD never advanced
            // chunk_start.
            inference::vad_last_speech_end(&mut model_mut.vad, tail)?.is_some()
        };
        if speechy {
            finalize_chunk(model_slot_mut(model)?, active, output)?;
        }
    }
    if abort.load(Ordering::Acquire) {
        // Cancelled while finalizing — do not emit Final/Metrics.
        *session = None;
        publish_active_session(active_session_id, None);
        abort.store(false, Ordering::Release);
        return Ok(());
    }
    let audio_ms = samples_to_ms(active.total_samples_received as usize);
    let processed_during_recording_ms = audio_ms.saturating_sub(tail_audio_ms);
    let session_id = active.id.clone();
    let first_partial_ms = active.first_partial_ms;
    let max_backlog_ms = active.max_backlog_ms;
    let audio_frames_dropped = active.audio_frames_dropped;
    let committed = active.committed.trim().to_string();
    let loaded = model_slot_mut(model)?;
    write_event(
        output,
        &AsrEvent::Metrics(atmospeak_asr_protocol::StreamingMetrics {
            session_id: session_id.clone(),
            backend: loaded.backend,
            model_id: loaded.model_id.clone(),
            first_partial_ms,
            stop_ack_ms: 0,
            finalize_ms: finalize_started.elapsed().as_millis() as u64,
            paste_ms: 0,
            processed_during_recording_ms,
            tail_audio_ms,
            max_backlog_ms,
            audio_frames_dropped,
            fallback_reason: None,
        }),
    )?;
    write_event(
        output,
        &AsrEvent::Final {
            session_id,
            text: committed,
            processed_during_recording_ms,
            tail_audio_ms,
        },
    )?;
    *session = None;
    publish_active_session(active_session_id, None);
    eprintln!(
        "finalized streaming session in {}ms",
        finalize_started.elapsed().as_millis()
    );
    Ok(())
}

fn emit_preview(model: &mut Model, session: &mut Session, output: &mut impl Write) -> Result<()> {
    // A preview is only a hypothesis for the uncommitted tail. Without this
    // clamp, the rolling window can include a committed chunk and visibly
    // repeat it in the dock.
    let start = session
        .audio
        .len()
        .saturating_sub(PREVIEW_SAMPLES)
        .max(session.chunk_start);
    let decoded = match inference::transcribe(
        &mut model.state,
        model.backend,
        model.threads,
        &session.audio[start..],
        &session.language,
        &prompt(session),
        session.profile,
        0,
        Some(&model.abort),
    ) {
        Ok(decoded) => decoded,
        Err(error) if model.abort.load(Ordering::Acquire) => {
            eprintln!("preview aborted: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    session.revision += 1;
    session.last_preview = Instant::now();
    let latency = session.started.elapsed().as_millis() as u64;
    session.first_partial_ms.get_or_insert(latency);
    write_event(
        output,
        &AsrEvent::Partial {
            session_id: session.id.clone(),
            revision: session.revision,
            text: decoded.text,
            covered_through_ms: samples_to_ms(
                session.audio_base_samples as usize + session.audio.len(),
            ),
        },
    )
}

fn finalize_chunk(model: &mut Model, session: &mut Session, output: &mut impl Write) -> Result<()> {
    let end = session.audio.len();
    let overlap_samples = session.chunk_start.min(OVERLAP_SAMPLES);
    let decode_start = session.chunk_start.saturating_sub(overlap_samples);
    let decoded = match inference::transcribe(
        &mut model.state,
        model.backend,
        model.threads,
        &session.audio[decode_start..end],
        &session.language,
        &prompt(session),
        session.profile,
        overlap_samples,
        Some(&model.abort),
    ) {
        Ok(decoded) => decoded,
        Err(error) if model.abort.load(Ordering::Acquire) => {
            // StopSession aborted this commit; leave chunk_start unchanged so
            // finalize_stopped_session still owns the uncommitted audio.
            eprintln!("commit aborted: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    // Whisper often invents a lone filler on trailing silence after a good
    // utterance ("The.", "[BLANK_AUDIO]"). Drop those; still advance the chunk
    // so StopSession does not re-decode the same silence.
    if !silence_hallucination(&decoded.text, &session.committed) {
        session.committed = inference::merge_overlap(
            &session.committed,
            &decoded.text,
            decoded.confirmed_overlap_words,
        );
        let start_ms = samples_to_ms(session.audio_base_samples as usize + session.chunk_start);
        let end_ms = samples_to_ms(session.audio_base_samples as usize + end);
        write_event(
            output,
            &AsrEvent::StableSegment {
                session_id: session.id.clone(),
                index: session.segment_index,
                text: session.committed.clone(),
                start_ms,
                end_ms,
            },
        )?;
    }
    session.segment_index += 1;
    // Keep only the audio needed to bridge the next chunk. This bounds host
    // memory for arbitrarily long recordings while retaining the 500 ms
    // acoustic overlap used at the next boundary.
    let retain_from = end.saturating_sub(OVERLAP_SAMPLES);
    session.audio.drain(..retain_from);
    session.audio_base_samples += retain_from as u64;
    session.chunk_start = session.audio.len();
    // The retained overlap was already evaluated; wait for fresh audio before
    // scheduling another VAD pass.
    session.last_vad_check = session.audio.len();
    session.speech_active = false;
    Ok(())
}

fn silence_hallucination(decoded: &str, _committed: &str) -> bool {
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    // Only drop unambiguous silence markers — never discard real filler words
    // like "the"/"a" that can be legitimate VAD-boundary chunks.
    lower.contains("blank_audio")
        || lower.contains("[silence]")
        || lower == "silence"
        || lower == "silence."
        || lower == "."
}

fn prompt(session: &Session) -> String {
    let combined = format!("{} {}", session.prompt, session.committed);
    let words = combined.split_whitespace().collect::<Vec<_>>();
    let start = words.len().saturating_sub(160);
    words[start..].join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vad_cadence_requires_fresh_audio() {
        let mut session = Session::new(
            "s".to_string(),
            "en".to_string(),
            String::new(),
            TranscriptionProfile::Balanced,
        );
        assert!(!vad_check_due(&session));
        session.audio = vec![0.0; SAMPLE_RATE];
        session.last_vad_check = 0;
        assert!(vad_check_due(&session));
        session.last_vad_check = SAMPLE_RATE - VAD_CHECK_SAMPLES + 1;
        assert!(!vad_check_due(&session));
    }

    #[test]
    fn silence_hallucination_keeps_real_filler_words() {
        assert!(silence_hallucination("[BLANK_AUDIO]", "hello there friend again"));
        assert!(silence_hallucination(".", "hello there friend again"));
        assert!(!silence_hallucination("the", "hello there friend again"));
        assert!(!silence_hallucination("you", "hello there friend again"));
    }
}
