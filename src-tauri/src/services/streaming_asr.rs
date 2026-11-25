use std::{
    error::Error as StdError,
    fmt,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use atmospeak_asr_protocol::{
    AsrBackend, AsrCommand, AsrEvent, MAX_FRAME_SIZE, PROTOCOL_VERSION, TranscriptionProfile,
};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::LiveTranscriptEvent,
    services::{app_state::AppState, asr_host, proc},
};

pub struct StreamingAsr {
    // Audio and control have separate bounded queues. A full audio queue drops
    // that frame immediately; Stop/Cancel remain serviceable on their own
    // queue and Stop carries a barrier for the frames that were accepted.
    audio_writer: SyncSender<QueuedAudio>,
    writer_thread: thread::Thread,
    control_writer: SyncSender<WriterRequest>,
    events: Mutex<Receiver<AsrEvent>>,
    child: Mutex<Child>,
    sequence: Mutex<SequenceState>,
    backend: AsrBackend,
    language: String,
    first_partial_ms: Arc<AtomicU64>,
    session_started: Arc<Mutex<Option<Instant>>>,
    delivery_failed: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    _job: asr_host::job::KillOnClose,
}

const AUDIO_WRITER_QUEUE_CAPACITY: usize = 256;
const CONTROL_WRITER_QUEUE_CAPACITY: usize = 32;
const WRITER_POLL_INTERVAL: Duration = Duration::from_millis(10);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub struct StreamingAsrStartupSuperseded;

impl fmt::Display for StreamingAsrStartupSuperseded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("streaming ASR startup superseded")
    }
}

impl StdError for StreamingAsrStartupSuperseded {}

pub fn is_startup_superseded(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<StreamingAsrStartupSuperseded>()
        .is_some()
}

struct QueuedAudio {
    sequence: u64,
    frame: Vec<u8>,
}

struct WriterRequest {
    frame: Vec<u8>,
    /// Highest sequence accepted by the audio queue before this control
    /// request. The writer drains through it before writing Stop.
    audio_barrier: Option<u64>,
    /// Cancel intentionally discards pending audio because no Final may be
    /// emitted for the canceled session.
    discard_audio: bool,
    reset_audio_sequence: bool,
    shutdown: bool,
}

#[derive(Default)]
struct SequenceState {
    session_id: Option<String>,
    stop_queued: bool,
    next_sequence: u64,
    last_attempted: Option<u64>,
    last_enqueued: Option<u64>,
    dropped_frames: u64,
    accepting_audio: bool,
}

pub struct StreamingFinal {
    pub text: String,
    pub queue_wait_ms: u64,
    pub decode_ms: u64,
    pub processed_during_recording_ms: u64,
    pub tail_audio_ms: u64,
    pub max_backlog_ms: u64,
    pub audio_frames_dropped: u64,
}

impl StreamingAsr {
    pub fn spawn(
        app: AppHandle,
        executable: PathBuf,
        model_path: PathBuf,
        language: String,
        threads: u16,
        startup_is_current: impl Fn() -> bool,
    ) -> Result<Arc<Self>> {
        let mut command = Command::new(&executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(directory) = executable.parent() {
            command.current_dir(directory);
        }
        proc::hide_console(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start {}", executable.display()))?;
        #[cfg(target_os = "windows")]
        let job = asr_host::job::attach(&child)?;
        if !startup_is_current() {
            terminate_child_process(&mut child);
            return Err(StreamingAsrStartupSuperseded.into());
        }
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("streaming host stdin is unavailable"))?;
        let (audio_writer_tx, audio_writer_rx) =
            mpsc::sync_channel::<QueuedAudio>(AUDIO_WRITER_QUEUE_CAPACITY);
        let (control_writer_tx, control_writer_rx) =
            mpsc::sync_channel::<WriterRequest>(CONTROL_WRITER_QUEUE_CAPACITY);
        let writer = thread::Builder::new()
            .name("atmospeak-asr-writer".to_string())
            .spawn(move || run_writer(stdin, control_writer_rx, audio_writer_rx))
            .context("failed to start streaming command writer")?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("streaming host stdout is unavailable"))?;
        let (event_tx, event_rx) = mpsc::channel();
        let first_partial_ms = Arc::new(AtomicU64::new(0));
        let session_started = Arc::new(Mutex::new(None::<Instant>));
        let delivery_failed = Arc::new(AtomicBool::new(false));
        let reader_first_partial_ms = first_partial_ms.clone();
        let reader_session_started = session_started.clone();
        thread::Builder::new()
            .name("atmospeak-asr-events".to_string())
            .spawn(move || {
                read_events(&mut stdout, event_tx, |event| match &event {
                    AsrEvent::Partial {
                        session_id,
                        revision,
                        text,
                        covered_through_ms,
                    } => {
                        let latency = reader_session_started
                            .lock()
                            .as_ref()
                            .map(|started| started.elapsed().as_millis() as u64);
                        if let Some(latency) = latency {
                            let _ = reader_first_partial_ms.compare_exchange(
                                0,
                                latency.max(1),
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            );
                        }
                        let first_partial_latency_ms =
                            match reader_first_partial_ms.load(Ordering::Relaxed) {
                                0 => None,
                                value => Some(value),
                            };
                        let payload = app
                            .try_state::<AppState>()
                            .and_then(|state| {
                                state.live_paste.apply_partial(
                                    session_id,
                                    text,
                                    *covered_through_ms,
                                    first_partial_latency_ms,
                                    *revision,
                                )
                            })
                            .unwrap_or_else(|| LiveTranscriptEvent {
                                session_id: session_id.clone(),
                                revision: *revision,
                                stable_text: String::new(),
                                partial_text: text.clone(),
                                covered_through_ms: *covered_through_ms,
                                first_partial_latency_ms,
                            });
                        let _ = app.emit("atmospeak://live-transcript", payload);
                    }
                    AsrEvent::StableSegment {
                        session_id,
                        text,
                        end_ms,
                        ..
                    } => {
                        let first_partial_latency_ms =
                            match reader_first_partial_ms.load(Ordering::Relaxed) {
                                0 => None,
                                value => Some(value),
                            };
                        let payload = app
                            .try_state::<AppState>()
                            .and_then(|state| {
                                state.live_paste.apply_stable(
                                    session_id,
                                    text,
                                    *end_ms,
                                    first_partial_latency_ms,
                                )
                            })
                            .unwrap_or_else(|| LiveTranscriptEvent {
                                session_id: session_id.clone(),
                                revision: 0,
                                stable_text: text.clone(),
                                partial_text: String::new(),
                                covered_through_ms: *end_ms,
                                first_partial_latency_ms,
                            });
                        let _ = app.emit("atmospeak://live-transcript", payload);
                    }
                    _ => {}
                });
            })
            .context("failed to start streaming event reader")?;
        let backend = AsrBackend::Cpu;
        let host = Arc::new(Self {
            audio_writer: audio_writer_tx,
            writer_thread: writer.thread().clone(),
            control_writer: control_writer_tx,
            events: Mutex::new(event_rx),
            child: Mutex::new(child),
            sequence: Mutex::new(SequenceState::default()),
            backend,
            language,
            first_partial_ms,
            session_started,
            delivery_failed,
            #[cfg(target_os = "windows")]
            _job: job,
        });
        if !startup_is_current() {
            host.terminate_child();
            return Err(StreamingAsrStartupSuperseded.into());
        }
        host.send(&AsrCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
        })?;
        host.send(&AsrCommand::LoadModel {
            model_path: model_path.to_string_lossy().to_string(),
            backend,
            threads,
        })?;
        match await_startup_event(&host, &startup_is_current)? {
            AsrEvent::Ready { capabilities }
                if capabilities.protocol_version == PROTOCOL_VERSION =>
            {
                if !capabilities.streaming {
                    return Err(anyhow!("The loaded voice model does not stream."));
                }
                Ok(host)
            }
            AsrEvent::Ready { capabilities } => Err(anyhow!(
                "protocol mismatch: client={}, host={}",
                PROTOCOL_VERSION,
                capabilities.protocol_version
            )),
            AsrEvent::Error { message, .. } => Err(anyhow!(message)),
            event => Err(anyhow!(
                "unexpected streaming host startup event: {event:?}"
            )),
        }
    }

    pub fn start_session(
        &self,
        session_id: String,
        prompt: String,
        profile: TranscriptionProfile,
    ) -> Result<()> {
        let mut sequence = self.sequence.lock();
        self.send(&AsrCommand::StartSession {
            session_id: session_id.clone(),
            language: self.language.clone(),
            initial_prompt: prompt,
            profile,
        })?;
        self.first_partial_ms.store(0, Ordering::Relaxed);
        self.delivery_failed.store(false, Ordering::Release);
        *self.session_started.lock() = Some(Instant::now());
        // Publish the session only after Start is queued, under the same lock
        // used by audio, Stop, and Cancel from potentially detached workers.
        *sequence = SequenceState {
            session_id: Some(session_id),
            accepting_audio: true,
            ..SequenceState::default()
        };
        Ok(())
    }

    pub fn send_audio(
        &self,
        session_id: &str,
        timestamp_ms: u64,
        pcm_s16le: Vec<u8>,
    ) -> Result<()> {
        let mut command = AsrCommand::AudioFrame {
            session_id: session_id.to_string(),
            sequence: 0,
            timestamp_ms,
            pcm_s16le,
        };
        let mut sequence_state = self.sequence.lock();
        if sequence_state.session_id.as_deref() != Some(session_id) {
            bail!("audio belongs to a superseded streaming session");
        }
        if !sequence_state.accepting_audio {
            bail!("streaming session is not accepting audio");
        }
        let sequence = sequence_state.next_sequence;
        sequence_state.next_sequence = sequence.saturating_add(1);
        sequence_state.last_attempted = Some(sequence);
        if let AsrCommand::AudioFrame {
            sequence: command_sequence,
            ..
        } = &mut command
        {
            *command_sequence = sequence;
        }
        let frame = match encode_frame(&command) {
            Ok(frame) => frame,
            Err(error) => {
                sequence_state.dropped_frames = sequence_state.dropped_frames.saturating_add(1);
                self.delivery_failed.store(true, Ordering::Release);
                return Err(error);
            }
        };
        match self.audio_writer.try_send(QueuedAudio { sequence, frame }) {
            Ok(()) => {
                sequence_state.last_enqueued = Some(sequence);
                self.writer_thread.unpark();
                Ok(())
            }
            Err(mpsc::TrySendError::Full(_)) => {
                sequence_state.dropped_frames = sequence_state.dropped_frames.saturating_add(1);
                self.delivery_failed.store(true, Ordering::Release);
                Err(anyhow!("streaming host audio queue is full"))
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                sequence_state.dropped_frames = sequence_state.dropped_frames.saturating_add(1);
                self.delivery_failed.store(true, Ordering::Release);
                Err(anyhow!("streaming host audio queue is unavailable"))
            }
        }
    }

    /// Tell the host to reconcile the session. Sent as soon as capture has
    /// detached and the last frame is enqueued, so the sidecar decodes the
    /// uncommitted tail while the recorder finishes file teardown instead of
    /// only starting after it.
    pub fn request_stop(&self, session_id: &str) -> Result<()> {
        let mut sequence = self.sequence.lock();
        if sequence.session_id.as_deref() != Some(session_id) {
            bail!("stop belongs to a superseded streaming session");
        }
        sequence.accepting_audio = false;
        if sequence.stop_queued {
            return Ok(());
        }
        self.send_control(
            &AsrCommand::StopSession {
                session_id: session_id.to_string(),
                last_sequence: sequence.last_attempted,
            },
            sequence.last_enqueued,
            false,
        )?;
        sequence.stop_queued = true;
        Ok(())
    }

    pub fn await_final(&self, session_id: &str, timeout: Duration) -> Result<StreamingFinal> {
        self.await_final_until(session_id, timeout, None)
    }

    /// Await a final while allowing the engine's out-of-band cancellation flag
    /// to wake the wait immediately. The engine must pass the same flag it sets
    /// before enqueueing Cancel so cancellation cannot fall through to batch
    /// fallback after a streaming wait.
    pub fn await_final_cancellable(
        &self,
        session_id: &str,
        timeout: Duration,
        cancel_requested: &AtomicBool,
    ) -> Result<StreamingFinal> {
        self.await_final_until(session_id, timeout, Some(cancel_requested))
    }

    fn await_final_until(
        &self,
        session_id: &str,
        timeout: Duration,
        cancel_requested: Option<&AtomicBool>,
    ) -> Result<StreamingFinal> {
        let deadline = Instant::now() + timeout;
        let mut host_metrics: Option<atmospeak_asr_protocol::StreamingMetrics> = None;
        loop {
            if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                self.cancel_session(session_id);
                self.terminate_child();
                return Err(anyhow!("streaming finalization cancelled"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.cancel_session(session_id);
                self.terminate_child();
                return Err(anyhow!(
                    "streaming finalization exceeded its {} ms deadline",
                    timeout.as_millis()
                ));
            }
            let wait =
                cancel_requested.map_or(remaining, |_| remaining.min(Duration::from_millis(25)));
            let received = self.events.lock().recv_timeout(wait);
            let event = match received {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(error) => {
                    if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                        self.cancel_session(session_id);
                        self.terminate_child();
                        return Err(anyhow!("streaming finalization cancelled"));
                    }
                    self.cancel_session(session_id);
                    self.terminate_child();
                    return Err(anyhow!("streaming host disconnected before Final: {error}"));
                }
            };
            if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                self.cancel_session(session_id);
                self.terminate_child();
                return Err(anyhow!("streaming finalization cancelled"));
            }
            match event {
                AsrEvent::Final {
                    session_id: completed,
                    text,
                    processed_during_recording_ms,
                    tail_audio_ms,
                } if completed == session_id => {
                    let Some(metrics) = host_metrics.as_ref() else {
                        self.cancel_session(session_id);
                        return Err(anyhow!(
                            "streaming final missing matching metrics; refusing unverified transcript"
                        ));
                    };
                    return Ok(StreamingFinal {
                        text,
                        queue_wait_ms: metrics.stop_ack_ms,
                        decode_ms: metrics.finalize_ms,
                        processed_during_recording_ms,
                        tail_audio_ms,
                        max_backlog_ms: metrics.max_backlog_ms,
                        audio_frames_dropped: reported_dropped_frames(
                            host_metrics.as_ref(),
                            self.sequence.lock().dropped_frames,
                        ),
                    });
                }
                AsrEvent::Metrics(metrics) if metrics.session_id == session_id => {
                    host_metrics = Some(metrics);
                }
                AsrEvent::Error {
                    session_id: failed,
                    message,
                    ..
                } if failed.as_deref().is_none_or(|failed| failed == session_id) => {
                    self.cancel_session(session_id);
                    return Err(anyhow!(message));
                }
                _ => {}
            }
        }
    }

    pub fn cancel_session(&self, session_id: &str) {
        let mut sequence = self.sequence.lock();
        if sequence.session_id.as_deref() != Some(session_id) {
            return;
        }
        sequence.accepting_audio = false;
        sequence.session_id = None;
        let _ = self.send_control(
            &AsrCommand::CancelSession {
                session_id: session_id.to_string(),
            },
            None,
            true,
        );
    }

    pub fn is_alive(&self) -> bool {
        matches!(self.child.lock().try_wait(), Ok(None))
    }

    pub fn backend(&self) -> AsrBackend {
        self.backend
    }

    pub fn first_partial_ms(&self) -> Option<u64> {
        match self.first_partial_ms.load(Ordering::Relaxed) {
            0 => None,
            value => Some(value),
        }
    }

    /// Sticky while any audio frame fails to reach the host. This is available
    /// before the writer drains its queue, so preview callers can fall back to
    /// finalization instead of releasing a truncated hypothesis.
    pub fn delivery_failed(&self) -> bool {
        self.delivery_failed.load(Ordering::Acquire)
    }

    pub fn shutdown(&self) {
        let _ = self.send(&AsrCommand::Shutdown);
        self.terminate_child();
    }

    fn send(&self, command: &AsrCommand) -> Result<()> {
        self.send_control(command, None, false)
    }

    fn send_control(
        &self,
        command: &AsrCommand,
        audio_barrier: Option<u64>,
        discard_audio: bool,
    ) -> Result<()> {
        let frame = encode_frame(command)?;
        self.control_writer
            .try_send(WriterRequest {
                frame,
                audio_barrier,
                discard_audio,
                reset_audio_sequence: matches!(command, AsrCommand::StartSession { .. }),
                shutdown: matches!(command, AsrCommand::Shutdown),
            })
            .map_err(|error| anyhow!("streaming host control queue is unavailable: {error}"))?;
        self.writer_thread.unpark();
        Ok(())
    }

    fn terminate_child(&self) {
        terminate_child_process(&mut self.child.lock());
    }

    fn recv_startup_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<AsrEvent, mpsc::RecvTimeoutError> {
        self.events.lock().recv_timeout(timeout)
    }
}

impl Drop for StreamingAsr {
    fn drop(&mut self) {
        terminate_child_process(self.child.get_mut());
    }
}

fn terminate_child_process(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn await_startup_event<F>(host: &StreamingAsr, startup_is_current: &F) -> Result<AsrEvent>
where
    F: Fn() -> bool,
{
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if !startup_is_current() {
            host.terminate_child();
            return Err(StreamingAsrStartupSuperseded.into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            host.terminate_child();
            bail!("streaming host startup timed out");
        }
        match host.recv_startup_timeout(remaining.min(STARTUP_POLL_INTERVAL)) {
            Ok(event) => return Ok(event),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("streaming host event reader disconnected during startup")
            }
        }
    }
}

fn encode_frame(command: &AsrCommand) -> Result<Vec<u8>> {
    let payload = rmp_serde::to_vec_named(command)?;
    if payload.is_empty() || payload.len() > MAX_FRAME_SIZE {
        bail!("invalid outgoing streaming frame length");
    }
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn reported_dropped_frames(
    metrics: Option<&atmospeak_asr_protocol::StreamingMetrics>,
    locally_dropped: u64,
) -> u64 {
    metrics
        .map(|metrics| metrics.audio_frames_dropped)
        .unwrap_or(0)
        .max(locally_dropped)
}

fn write_frame(output: &mut impl Write, frame: &[u8]) -> Result<()> {
    output.write_all(frame)?;
    output.flush()?;
    Ok(())
}

fn process_writer_request<W: Write>(
    output: &mut W,
    request: WriterRequest,
    audio_rx: &Receiver<QueuedAudio>,
    last_written: &mut Option<u64>,
    pending_audio: &mut Option<QueuedAudio>,
) -> Result<bool> {
    if request.discard_audio {
        *pending_audio = None;
        while audio_rx.try_recv().is_ok() {}
    }
    if request.reset_audio_sequence {
        *last_written = None;
    }
    if let Some(barrier) = request.audio_barrier {
        while last_written.is_none_or(|written| written < barrier) {
            match pending_audio
                .take()
                .map(Ok)
                .unwrap_or_else(|| audio_rx.recv_timeout(WRITER_POLL_INTERVAL))
            {
                Ok(audio) => {
                    write_frame(output, &audio.frame)?;
                    *last_written = Some(audio.sequence);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
    write_frame(output, &request.frame)?;
    Ok(request.shutdown)
}

fn run_writer<W: Write>(
    mut output: W,
    control_rx: Receiver<WriterRequest>,
    audio_rx: Receiver<QueuedAudio>,
) -> Result<W> {
    let mut last_written = None::<u64>;
    let mut audio_closed = false;
    let mut pending_audio = None;
    loop {
        match control_rx.try_recv() {
            Ok(request) => {
                if process_writer_request(
                    &mut output,
                    request,
                    &audio_rx,
                    &mut last_written,
                    &mut pending_audio,
                )? {
                    return Ok(output);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                if let Some(audio) = pending_audio.take() {
                    write_frame(&mut output, &audio.frame)?;
                    last_written = Some(audio.sequence);
                    continue;
                }
                if audio_closed {
                    match control_rx.recv() {
                        Ok(request) => {
                            if process_writer_request(
                                &mut output,
                                request,
                                &audio_rx,
                                &mut last_written,
                                &mut pending_audio,
                            )? {
                                return Ok(output);
                            }
                        }
                        Err(_) => return Ok(output),
                    }
                    continue;
                }
                match audio_rx.try_recv() {
                    Ok(audio) => {
                        // Recheck control before writing the first frame, or
                        // the sidecar can receive audio before its session.
                        pending_audio = Some(audio);
                    }
                    // Producers unpark after enqueue. The park token preserves
                    // notifications sent between these empty reads and parking.
                    Err(mpsc::TryRecvError::Empty) => thread::park_timeout(WRITER_POLL_INTERVAL),
                    Err(mpsc::TryRecvError::Disconnected) => audio_closed = true,
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => return Ok(output),
        }
    }
}

fn read_frame<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> Result<Option<T>> {
    let mut length = [0_u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_SIZE {
        bail!("invalid incoming streaming frame length");
    }
    let mut payload = vec![0; length];
    input.read_exact(&mut payload)?;
    Ok(Some(rmp_serde::from_slice(&payload)?))
}

/// Preview callbacks run immediately; completion consumes lifecycle events.
fn read_events(
    input: &mut impl Read,
    events: mpsc::Sender<AsrEvent>,
    mut preview: impl FnMut(&AsrEvent),
) {
    while let Ok(Some(event)) = read_frame::<AsrEvent>(input) {
        if matches!(
            event,
            AsrEvent::Partial { .. } | AsrEvent::StableSegment { .. }
        ) {
            preview(&event);
            // Preview consumers already own the latest state. Finalization
            // never uses historical preview copies.
            continue;
        }
        if events.send(event).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn previews_do_not_accumulate_while_finalization_is_idle() {
        let mut wire = Vec::new();
        let mut text = String::new();
        for revision in 1..=1_800 {
            text.push_str("a growing phrase");
            let payload = rmp_serde::to_vec_named(&AsrEvent::Partial {
                session_id: "session".into(),
                revision,
                text: text.clone(),
                covered_through_ms: revision * 1_000,
            })
            .unwrap();
            wire.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            wire.extend_from_slice(&payload);
        }
        let stable = AsrEvent::StableSegment {
            session_id: "session".into(),
            index: 0,
            text: text.clone(),
            start_ms: 0,
            end_ms: 1_800_000,
        };
        let expected = vec![
            AsrEvent::Ready {
                capabilities: atmospeak_asr_protocol::AsrCapabilities {
                    protocol_version: PROTOCOL_VERSION,
                    backend: AsrBackend::Cpu,
                    streaming: true,
                    vad: true,
                },
            },
            AsrEvent::Error {
                session_id: Some("session".into()),
                recoverable: true,
                message: "diagnostic".into(),
            },
            AsrEvent::Metrics(atmospeak_asr_protocol::StreamingMetrics {
                session_id: "session".into(),
                backend: AsrBackend::Cpu,
                model_id: "model".into(),
                first_partial_ms: Some(10),
                stop_ack_ms: 1,
                finalize_ms: 2,
                paste_ms: 0,
                processed_during_recording_ms: 1_800_000,
                tail_audio_ms: 0,
                max_backlog_ms: 0,
                audio_frames_dropped: 0,
                fallback_reason: None,
            }),
            AsrEvent::Final {
                session_id: "session".into(),
                text: text.clone(),
                processed_during_recording_ms: 1_800_000,
                tail_audio_ms: 0,
            },
        ];
        for event in std::iter::once(&stable).chain(expected.iter()) {
            let payload = rmp_serde::to_vec_named(event).unwrap();
            wire.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            wire.extend_from_slice(&payload);
        }
        let (tx, rx) = mpsc::channel();
        let mut displayed = 0;
        let mut latest = String::new();
        read_events(&mut Cursor::new(wire), tx, |event| match event {
            AsrEvent::Partial { text, .. } | AsrEvent::StableSegment { text, .. } => {
                displayed += 1;
                latest.clone_from(text);
            }
            _ => panic!("lifecycle event reached preview callback"),
        });
        let queued = rx.into_iter().collect::<Vec<_>>();
        let preview_bytes = queued
            .iter()
            .map(|event| match event {
                AsrEvent::Partial { text, .. } | AsrEvent::StableSegment { text, .. } => text.len(),
                _ => 0,
            })
            .sum::<usize>();
        eprintln!(
            "preview queue: {} events, {} retained preview-text bytes; {} displayed",
            queued.len(),
            preview_bytes,
            displayed
        );
        assert_eq!(displayed, 1_801);
        assert_eq!(latest, text);
        assert_eq!(
            queued.len(),
            expected.len(),
            "undisplayed completion backlog"
        );
        assert_eq!(
            queued, expected,
            "lifecycle order and final text must survive"
        );
    }

    #[test]
    fn stop_barrier_writes_accepted_audio_before_control() {
        let (control_tx, control_rx) = mpsc::sync_channel(4);
        let (audio_tx, audio_rx) = mpsc::sync_channel(4);
        audio_tx
            .send(QueuedAudio {
                sequence: 0,
                frame: vec![b'a'],
            })
            .unwrap();
        audio_tx
            .send(QueuedAudio {
                sequence: 1,
                frame: vec![b'b'],
            })
            .unwrap();
        control_tx
            .send(WriterRequest {
                frame: vec![b's'],
                audio_barrier: Some(1),
                discard_audio: false,
                reset_audio_sequence: false,
                shutdown: false,
            })
            .unwrap();
        control_tx
            .send(WriterRequest {
                frame: vec![b'x'],
                audio_barrier: None,
                discard_audio: false,
                reset_audio_sequence: false,
                shutdown: true,
            })
            .unwrap();

        let output = run_writer(Cursor::new(Vec::new()), control_rx, audio_rx).expect("writer");
        assert_eq!(output.into_inner(), b"absx");

        let (control_tx, control_rx) = mpsc::sync_channel(4);
        let (audio_tx, audio_rx) = mpsc::sync_channel(4);
        let writer =
            thread::spawn(move || run_writer(Cursor::new(Vec::new()), control_rx, audio_rx));
        thread::sleep(Duration::from_millis(5));
        control_tx
            .send(WriterRequest {
                frame: vec![b'n'],
                audio_barrier: None,
                discard_audio: false,
                reset_audio_sequence: true,
                shutdown: false,
            })
            .unwrap();
        audio_tx
            .send(QueuedAudio {
                sequence: 0,
                frame: vec![b'a'],
            })
            .unwrap();
        control_tx
            .send(WriterRequest {
                frame: vec![b's'],
                audio_barrier: Some(0),
                discard_audio: false,
                reset_audio_sequence: false,
                shutdown: false,
            })
            .unwrap();
        control_tx
            .send(WriterRequest {
                frame: vec![b'x'],
                audio_barrier: None,
                discard_audio: false,
                reset_audio_sequence: false,
                shutdown: true,
            })
            .unwrap();
        drop(audio_tx);
        assert_eq!(writer.join().unwrap().unwrap().into_inner(), b"nasx");

        // Audio can arrive while the writer is asleep waiting on its queue.
        // Start still precedes that held frame, and resets the previous
        // session's sequence before Stop drains the new session.
        let (audio_tx, audio_rx) = mpsc::sync_channel(2);
        audio_tx
            .send(QueuedAudio {
                sequence: 1,
                frame: vec![b'b'],
            })
            .unwrap();
        let mut pending = Some(QueuedAudio {
            sequence: 0,
            frame: vec![b'a'],
        });
        let mut last_written = Some(999);
        let mut output = Cursor::new(Vec::new());
        process_writer_request(
            &mut output,
            WriterRequest {
                frame: vec![b'n'],
                audio_barrier: None,
                discard_audio: false,
                reset_audio_sequence: true,
                shutdown: false,
            },
            &audio_rx,
            &mut last_written,
            &mut pending,
        )
        .unwrap();
        process_writer_request(
            &mut output,
            WriterRequest {
                frame: vec![b's'],
                audio_barrier: Some(1),
                discard_audio: false,
                reset_audio_sequence: false,
                shutdown: false,
            },
            &audio_rx,
            &mut last_written,
            &mut pending,
        )
        .unwrap();
        assert_eq!(output.into_inner(), b"nabs");
        assert!(pending.is_none());
    }

    #[test]
    fn cancel_discards_pending_audio_and_remains_immediate() {
        let (control_tx, control_rx) = mpsc::sync_channel(4);
        let (audio_tx, audio_rx) = mpsc::sync_channel(4);
        audio_tx
            .send(QueuedAudio {
                sequence: 0,
                frame: vec![b'a'],
            })
            .unwrap();
        audio_tx
            .send(QueuedAudio {
                sequence: 1,
                frame: vec![b'b'],
            })
            .unwrap();
        control_tx
            .send(WriterRequest {
                frame: vec![b'c'],
                audio_barrier: None,
                discard_audio: true,
                reset_audio_sequence: false,
                shutdown: false,
            })
            .unwrap();
        control_tx
            .send(WriterRequest {
                frame: vec![b'x'],
                audio_barrier: None,
                discard_audio: false,
                reset_audio_sequence: false,
                shutdown: true,
            })
            .unwrap();

        let output = run_writer(Cursor::new(Vec::new()), control_rx, audio_rx).expect("writer");
        assert_eq!(output.into_inner(), b"cx");
        let (_audio_tx, audio_rx) = mpsc::sync_channel(1);
        let mut pending = Some(QueuedAudio {
            sequence: 0,
            frame: vec![b'a'],
        });
        let mut last_written = None;
        let mut output = Cursor::new(Vec::new());
        process_writer_request(
            &mut output,
            WriterRequest {
                frame: vec![b'c'],
                audio_barrier: None,
                discard_audio: true,
                reset_audio_sequence: false,
                shutdown: false,
            },
            &audio_rx,
            &mut last_written,
            &mut pending,
        )
        .unwrap();
        assert_eq!(output.into_inner(), b"c");
        assert!(pending.is_none());
    }

    #[test]
    fn local_drop_count_is_sticky_until_the_next_session() {
        let mut state = SequenceState {
            accepting_audio: true,
            ..SequenceState::default()
        };
        state.next_sequence = 4;
        state.last_attempted = Some(3);
        state.dropped_frames = 2;
        assert_eq!(state.last_attempted, Some(3));
        assert_eq!(state.dropped_frames, 2);
        state = SequenceState {
            accepting_audio: true,
            ..SequenceState::default()
        };
        assert_eq!(state.dropped_frames, 0);
    }

    #[test]
    fn final_drop_count_keeps_the_larger_host_or_local_total() {
        let metrics = atmospeak_asr_protocol::StreamingMetrics {
            session_id: "s".to_string(),
            backend: AsrBackend::Cpu,
            model_id: "model".to_string(),
            first_partial_ms: None,
            stop_ack_ms: 0,
            finalize_ms: 0,
            paste_ms: 0,
            processed_during_recording_ms: 0,
            tail_audio_ms: 0,
            max_backlog_ms: 0,
            audio_frames_dropped: 3,
            fallback_reason: None,
        };
        assert_eq!(reported_dropped_frames(Some(&metrics), 5), 5);
        assert_eq!(reported_dropped_frames(Some(&metrics), 1), 3);
        assert_eq!(reported_dropped_frames(None, 2), 2);
    }

    #[test]
    fn session_controls_are_fenced_and_unverified_final_is_rejected() {
        let mut command = if cfg!(target_os = "windows") {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleeping child");
        #[cfg(target_os = "windows")]
        let job = asr_host::job::attach(&child).expect("kill-on-close job");
        let (_audio_tx, _audio_rx) = mpsc::sync_channel(AUDIO_WRITER_QUEUE_CAPACITY);
        let (_control_tx, _control_rx) = mpsc::sync_channel(CONTROL_WRITER_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel();
        let host = StreamingAsr {
            audio_writer: _audio_tx,
            writer_thread: thread::current(),
            control_writer: _control_tx,
            events: Mutex::new(event_rx),
            child: Mutex::new(child),
            sequence: Mutex::new(SequenceState::default()),
            backend: AsrBackend::Cpu,
            first_partial_ms: Arc::new(AtomicU64::new(0)),
            session_started: Arc::new(Mutex::new(None)),
            delivery_failed: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            _job: job,
        };
        host.start_session(
            "session".into(),
            String::new(),
            TranscriptionProfile::Balanced,
        )
        .unwrap();
        let _ = _control_rx.recv().unwrap();
        assert!(host.send_audio("old-session", 0, vec![0, 0]).is_err());
        assert!(host.request_stop("old-session").is_err());
        host.cancel_session("old-session");
        assert!(host.sequence.lock().accepting_audio);
        for _ in 0..CONTROL_WRITER_QUEUE_CAPACITY {
            host.control_writer
                .try_send(WriterRequest {
                    frame: Vec::new(),
                    audio_barrier: None,
                    discard_audio: false,
                    reset_audio_sequence: false,
                    shutdown: false,
                })
                .unwrap();
        }
        assert!(
            host.start_session(
                "blocked-session".into(),
                String::new(),
                TranscriptionProfile::Balanced
            )
            .is_err()
        );
        assert_eq!(host.sequence.lock().session_id.as_deref(), Some("session"));
        assert!(host.request_stop("session").is_err());
        assert!(!host.sequence.lock().stop_queued);
        while _control_rx.try_recv().is_ok() {}
        // Failed Stop closes ingress, but remains retryable after queue recovery.
        assert!(host.send_audio("session", 0, vec![0, 0]).is_err());
        host.request_stop("session").unwrap();
        assert!(host.sequence.lock().stop_queued);
        let _ = _control_rx.recv().unwrap();
        host.start_session(
            "session".into(),
            String::new(),
            TranscriptionProfile::Balanced,
        )
        .unwrap();
        let _ = _control_rx.recv().unwrap();
        host.send_audio("session", 0, vec![0, 0]).unwrap();
        assert_eq!(host.sequence.lock().last_enqueued, Some(0));
        host.request_stop("session").unwrap();
        assert!(host.send_audio("session", 20, vec![0, 0]).is_err());
        host.request_stop("session").unwrap();
        let request = _control_rx.recv().unwrap();
        assert!(matches!(
            read_frame::<AsrCommand>(&mut Cursor::new(request.frame)).unwrap(),
            Some(AsrCommand::StopSession { .. })
        ));
        assert_eq!(request.audio_barrier, Some(0));
        assert!(_control_rx.try_recv().is_err());
        host.start_session(
            "next-session".into(),
            String::new(),
            TranscriptionProfile::Balanced,
        )
        .unwrap();
        let _ = _control_rx.recv().unwrap();
        host.cancel_session("session");
        assert!(host.send_audio("session", 40, vec![0, 0]).is_err());
        assert!(host.request_stop("session").is_err());
        host.send_audio("next-session", 0, vec![0, 0]).unwrap();
        assert_eq!(host.sequence.lock().last_enqueued, Some(0));
        event_tx
            .send(AsrEvent::Final {
                session_id: "next-session".to_string(),
                text: "unverified".to_string(),
                processed_during_recording_ms: 0,
                tail_audio_ms: 0,
            })
            .expect("final event");

        let error = host
            .await_final("next-session", Duration::from_secs(1))
            .err()
            .expect("final without metrics must not be released");
        assert!(error.to_string().contains("missing matching metrics"));
        host.terminate_child();
        assert!(
            host.child
                .lock()
                .try_wait()
                .expect("child status")
                .is_some(),
            "test host cleanup"
        );
    }

    #[test]
    fn await_final_terminates_a_wedged_child_within_the_deadline() {
        let mut command = if cfg!(target_os = "windows") {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleeping child");
        #[cfg(target_os = "windows")]
        let job = asr_host::job::attach(&child).expect("kill-on-close job");
        let (_audio_tx, _audio_rx) = mpsc::sync_channel(AUDIO_WRITER_QUEUE_CAPACITY);
        let (_control_tx, _control_rx) = mpsc::sync_channel(CONTROL_WRITER_QUEUE_CAPACITY);
        let (_event_tx, event_rx) = mpsc::channel();
        let host = StreamingAsr {
            audio_writer: _audio_tx,
            writer_thread: thread::current(),
            control_writer: _control_tx,
            events: Mutex::new(event_rx),
            child: Mutex::new(child),
            sequence: Mutex::new(SequenceState::default()),
            backend: AsrBackend::Cpu,
            first_partial_ms: Arc::new(AtomicU64::new(0)),
            session_started: Arc::new(Mutex::new(None)),
            delivery_failed: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            _job: job,
        };
        let started = Instant::now();
        let result = host.await_final("session", Duration::from_millis(20));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            host.child
                .lock()
                .try_wait()
                .expect("child status")
                .is_some(),
            "timeout must terminate the wedged child"
        );

        // Keep the compiler from treating this as a test-only process leak if
        // a platform's kill implementation reports an already-reaped child.
        let _ = host.child.lock().wait();
    }

    #[test]
    fn cancellable_await_wakes_before_the_normal_deadline() {
        let mut command = if cfg!(target_os = "windows") {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleeping child");
        #[cfg(target_os = "windows")]
        let job = asr_host::job::attach(&child).expect("kill-on-close job");
        let (_audio_tx, _audio_rx) = mpsc::sync_channel(AUDIO_WRITER_QUEUE_CAPACITY);
        let (_control_tx, _control_rx) = mpsc::sync_channel(CONTROL_WRITER_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel();
        let host = StreamingAsr {
            audio_writer: _audio_tx,
            writer_thread: thread::current(),
            control_writer: _control_tx,
            events: Mutex::new(event_rx),
            child: Mutex::new(child),
            sequence: Mutex::new(SequenceState::default()),
            backend: AsrBackend::Cpu,
            first_partial_ms: Arc::new(AtomicU64::new(0)),
            session_started: Arc::new(Mutex::new(None)),
            delivery_failed: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            _job: job,
        };
        let delayed = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            let metrics = atmospeak_asr_protocol::StreamingMetrics {
                session_id: "session".into(),
                backend: AsrBackend::Cpu,
                model_id: "model".into(),
                first_partial_ms: None,
                stop_ack_ms: 7,
                finalize_ms: 80,
                paste_ms: 0,
                processed_during_recording_ms: 1000,
                tail_audio_ms: 80,
                max_backlog_ms: 0,
                audio_frames_dropped: 0,
                fallback_reason: None,
            };
            if event_tx.send(AsrEvent::Metrics(metrics)).is_ok() {
                let _ = event_tx.send(AsrEvent::Final {
                    session_id: "session".into(),
                    text: "Complete text".into(),
                    processed_during_recording_ms: 1000,
                    tail_audio_ms: 80,
                });
            }
        });
        let not_cancelled = AtomicBool::new(false);
        let complete =
            host.await_final_cancellable("session", Duration::from_secs(1), &not_cancelled);
        delayed.join().unwrap();
        let complete =
            complete.expect("an idle cancellation poll is not a finalization deadline");
        assert_eq!(complete.text, "Complete text");
        assert_eq!(complete.queue_wait_ms, 7);
        assert_eq!(complete.decode_ms, 80);

        let cancel_requested = AtomicBool::new(true);
        let started = Instant::now();
        let result =
            host.await_final_cancellable("session", Duration::from_secs(120), &cancel_requested);

        let error = match result {
            Ok(_) => panic!("cancellation must not produce a final"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            host.child
                .lock()
                .try_wait()
                .expect("child status")
                .is_some(),
            "cancellation must terminate the child"
        );
    }

    #[test]
    fn startup_wait_terminates_owned_child_when_generation_is_superseded() {
        let mut command = if cfg!(target_os = "windows") {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleeping child");
        #[cfg(target_os = "windows")]
        let job = asr_host::job::attach(&child).expect("kill-on-close job");
        let (_audio_tx, _audio_rx) = mpsc::sync_channel(AUDIO_WRITER_QUEUE_CAPACITY);
        let (_control_tx, _control_rx) = mpsc::sync_channel(CONTROL_WRITER_QUEUE_CAPACITY);
        let (_event_tx, event_rx) = mpsc::channel();
        let host = StreamingAsr {
            audio_writer: _audio_tx,
            writer_thread: thread::current(),
            control_writer: _control_tx,
            events: Mutex::new(event_rx),
            child: Mutex::new(child),
            sequence: Mutex::new(SequenceState::default()),
            backend: AsrBackend::Cpu,
            first_partial_ms: Arc::new(AtomicU64::new(0)),
            session_started: Arc::new(Mutex::new(None)),
            delivery_failed: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            _job: job,
        };
        let current = Arc::new(AtomicBool::new(true));
        let supersede = current.clone();
        let flipper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            supersede.store(false, Ordering::Release);
        });
        let started = Instant::now();
        let startup_is_current = || current.load(Ordering::Acquire);
        let error = await_startup_event(&host, &startup_is_current)
            .err()
            .expect("superseded startup must fail");
        flipper.join().expect("supersession thread");

        assert!(is_startup_superseded(&error));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            host.child
                .lock()
                .try_wait()
                .expect("child status")
                .is_some(),
            "superseded startup must terminate its owned child"
        );
    }
}
