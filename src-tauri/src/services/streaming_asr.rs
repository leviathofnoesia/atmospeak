use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
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
use tauri::{AppHandle, Emitter};
use tauri::{Manager, path::BaseDirectory};

use crate::{
    models::LiveTranscriptEvent,
    services::{app_state::AppState, asr_host, proc},
};

pub struct StreamingAsr {
    // A dedicated writer owns the blocking pipe. Public operations only enqueue
    // bounded frames, so a wedged child cannot hold a mutex and prevent cancel
    // or shutdown from recovering it.
    writer: SyncSender<Vec<u8>>,
    events: Mutex<Receiver<AsrEvent>>,
    child: Mutex<Child>,
    sequence: AtomicU64,
    backend: AsrBackend,
    first_partial_ms: Arc<AtomicU64>,
    session_started: Arc<Mutex<Option<Instant>>>,
    #[cfg(target_os = "windows")]
    _job: asr_host::job::KillOnClose,
}

pub struct StreamingFinal {
    pub text: String,
    pub processed_during_recording_ms: u64,
    pub tail_audio_ms: u64,
    pub max_backlog_ms: u64,
    pub audio_frames_dropped: u64,
}

pub fn resolve_executable(app: &AppHandle, backend: AsrBackend) -> Option<PathBuf> {
    let filename = match backend {
        AsrBackend::Vulkan => "atmospeak-asr-vulkan.exe",
        _ => "atmospeak-asr-cpu.exe",
    };
    let resource = format!("resources/asr/{filename}");
    app.path()
        .resolve(&resource, BaseDirectory::Resource)
        .ok()
        .filter(|path| path.is_file())
        .or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("asr")
                .join(filename)
                .is_file()
                .then(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("resources")
                        .join("asr")
                        .join(filename)
                })
        })
}

impl StreamingAsr {
    pub fn spawn(
        app: AppHandle,
        executable: PathBuf,
        model_path: PathBuf,
        backend: AsrBackend,
        threads: u16,
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("streaming host stdin is unavailable"))?;
        let (writer_tx, writer_rx) = mpsc::sync_channel::<Vec<u8>>(256);
        thread::Builder::new()
            .name("atmospeak-asr-writer".to_string())
            .spawn(move || {
                let mut stdin = stdin;
                while let Ok(frame) = writer_rx.recv() {
                    if stdin.write_all(&frame).is_err() || stdin.flush().is_err() {
                        break;
                    }
                }
            })
            .context("failed to start streaming command writer")?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("streaming host stdout is unavailable"))?;
        let (event_tx, event_rx) = mpsc::channel();
        let first_partial_ms = Arc::new(AtomicU64::new(0));
        let session_started = Arc::new(Mutex::new(None::<Instant>));
        let reader_first_partial_ms = first_partial_ms.clone();
        let reader_session_started = session_started.clone();
        thread::Builder::new()
            .name("atmospeak-asr-events".to_string())
            .spawn(move || {
                while let Ok(Some(event)) = read_frame::<AsrEvent>(&mut stdout) {
                    match &event {
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
                    }
                    if event_tx.send(event).is_err() {
                        break;
                    }
                }
            })
            .context("failed to start streaming event reader")?;
        let host = Arc::new(Self {
            writer: writer_tx,
            events: Mutex::new(event_rx),
            child: Mutex::new(child),
            sequence: AtomicU64::new(0),
            backend,
            first_partial_ms,
            session_started,
            #[cfg(target_os = "windows")]
            _job: job,
        });
        host.send(&AsrCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
        })?;
        host.send(&AsrCommand::LoadModel {
            model_path: model_path.to_string_lossy().to_string(),
            backend,
            threads,
        })?;
        match host.recv_timeout(Duration::from_secs(180))? {
            AsrEvent::Ready { .. } => Ok(host),
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
        self.sequence.store(0, Ordering::Relaxed);
        self.first_partial_ms.store(0, Ordering::Relaxed);
        *self.session_started.lock() = Some(Instant::now());
        self.send(&AsrCommand::StartSession {
            session_id,
            language: "en".to_string(),
            initial_prompt: prompt,
            profile,
        })
    }

    pub fn send_audio(
        &self,
        session_id: &str,
        timestamp_ms: u64,
        pcm_s16le: Vec<u8>,
    ) -> Result<()> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.send(&AsrCommand::AudioFrame {
            session_id: session_id.to_string(),
            sequence,
            timestamp_ms,
            pcm_s16le,
        })
    }

    /// Convenience wrapper kept for callers that do not need to overlap other
    /// work with the host's finalize. Prefer `request_stop` + `await_final`
    /// when capture teardown can run in parallel with the tail decode.
    #[allow(dead_code)]
    pub fn stop_session(&self, session_id: &str, timeout: Duration) -> Result<StreamingFinal> {
        self.request_stop(session_id)?;
        self.await_final(session_id, timeout)
    }

    /// Tell the host to reconcile the session. Sent as soon as capture has
    /// detached and the last frame is enqueued, so the sidecar decodes the
    /// uncommitted tail while the recorder finishes file teardown instead of
    /// only starting after it.
    pub fn request_stop(&self, session_id: &str) -> Result<()> {
        self.send(&AsrCommand::StopSession {
            session_id: session_id.to_string(),
        })
    }

    pub fn await_final(&self, session_id: &str, timeout: Duration) -> Result<StreamingFinal> {
        let deadline = Instant::now() + timeout;
        let mut host_metrics = None;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let event = match self.recv_timeout(remaining) {
                Ok(event) => event,
                Err(error) => {
                    self.cancel_session(session_id);
                    return Err(error);
                }
            };
            match event {
                AsrEvent::Final {
                    session_id: completed,
                    text,
                    processed_during_recording_ms,
                    tail_audio_ms,
                } if completed == session_id => {
                    return Ok(StreamingFinal {
                        text,
                        processed_during_recording_ms,
                        tail_audio_ms,
                        max_backlog_ms: host_metrics
                            .as_ref()
                            .map(|metrics: &atmospeak_asr_protocol::StreamingMetrics| {
                                metrics.max_backlog_ms
                            })
                            .unwrap_or(0),
                        audio_frames_dropped: host_metrics
                            .as_ref()
                            .map(|metrics| metrics.audio_frames_dropped)
                            .unwrap_or(0),
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
        let _ = self.send(&AsrCommand::CancelSession {
            session_id: session_id.to_string(),
        });
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

    pub fn shutdown(&self) {
        let _ = self.send(&AsrCommand::Shutdown);
        let _ = self.child.lock().kill();
        let _ = self.child.lock().wait();
    }

    fn send(&self, command: &AsrCommand) -> Result<()> {
        let payload = rmp_serde::to_vec_named(command)?;
        if payload.is_empty() || payload.len() > MAX_FRAME_SIZE {
            bail!("invalid outgoing streaming frame length");
        }
        let mut frame = Vec::with_capacity(payload.len() + 4);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);
        self.writer
            .try_send(frame)
            .map_err(|error| anyhow!("streaming host command queue is unavailable: {error}"))
    }

    fn recv_timeout(&self, timeout: Duration) -> Result<AsrEvent> {
        self.events
            .lock()
            .recv_timeout(timeout)
            .map_err(|error| anyhow!("streaming host event timeout: {error}"))
    }
}

impl Drop for StreamingAsr {
    fn drop(&mut self) {
        let _ = self.child.get_mut().kill();
        let _ = self.child.get_mut().wait();
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
