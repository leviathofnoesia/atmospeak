use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use cpal::{
    SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use hound::{SampleFormat as WavSampleFormat, WavReader, WavSpec, WavWriter};
use parking_lot::Mutex;
use uuid::Uuid;

mod journal;
mod wav_recovery;

use journal::{
    CaptureJournal, checkpoint_journal_complete, clear_discard_marker, journal_path_for_recording,
    read_journal, write_discard_marker,
};
use wav_recovery::{
    acknowledge_recovered_recording, list_recoverable_recordings, recover_recording,
    recover_wav_from_partial, wav_data_sample_count,
};

use crate::{
    models::{MicLevel, MicrophoneInfo, RecordingStarted, TranscriptionProfile},
    services::streaming_asr::StreamingAsr,
};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const CHECKPOINT_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const JOURNAL_EXTENSION: &str = "journal";
const DISCARD_MARKER_EXTENSION: &str = "discarded";
const WAV_HEADER_BYTES: u64 = 44;
const WAV_SAMPLE_BYTES: u64 = 2;
const STREAMING_WORKER_STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub struct RecorderService {
    recordings_dir: PathBuf,
    active: Mutex<Option<ActiveRecording>>,
}

struct ActiveRecording {
    id: String,
    started_instant: Instant,
    sample_rate: u32,
    samples: Arc<Mutex<VecDeque<f32>>>,
    duration_override_ms: Option<u64>,
    _stream: Option<Stream>,
    streaming: Option<ActiveStreaming>,
}

pub struct StreamingStart {
    pub host: Arc<StreamingAsr>,
    pub prompt: String,
    pub profile: TranscriptionProfile,
}

struct ActiveStreaming {
    host: Option<Arc<StreamingAsr>>,
    sender: Option<StreamingFrameSender>,
    worker: Option<thread::JoinHandle<()>>,
    worker_done: Arc<AtomicBool>,
    stop_requested: Arc<AtomicBool>,
    discard_requested: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    send_failed: Arc<AtomicBool>,
    write_failed: Arc<AtomicBool>,
    wav_ready: Arc<AtomicBool>,
}

struct InputFrame {
    sample_rate: u32,
    samples: Vec<f32>,
}

#[derive(Clone)]
struct StreamingFrameSender {
    tx: SyncSender<InputFrame>,
    dropped: Arc<AtomicU64>,
    capture_failed: Arc<AtomicBool>,
}

/// A capture that survived an interrupted native process and can be surfaced
/// to History or recovered into the normal WAV path on the next startup.
#[derive(Debug, Clone)]
pub struct RecoverableRecording {
    pub id: String,
    pub path: PathBuf,
    pub journal_path: PathBuf,
    pub sample_rate: u32,
    pub sample_count: u64,
    pub duration_ms: u64,
    pub materialized: bool,
}

pub struct FinishedRecording {
    pub id: String,
    pub path: PathBuf,
    pub duration_ms: u64,
}

pub struct CapturedRecording {
    pub id: String,
    pub path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub streaming_host: Option<Arc<StreamingAsr>>,
    pub streaming_frames_dropped: u64,
    pending_samples: Option<Arc<Mutex<VecDeque<f32>>>>,
    streaming_worker: Option<thread::JoinHandle<()>>,
    streaming_worker_done: Option<Arc<AtomicBool>>,
    streaming_stop_requested: Option<Arc<AtomicBool>>,
    streaming_dropped: Option<Arc<AtomicU64>>,
    streaming_send_failed: Option<Arc<AtomicBool>>,
    streaming_write_failed: Option<Arc<AtomicBool>>,
    streaming_wav_ready: Option<Arc<AtomicBool>>,
    audio_prepared: bool,
}

impl CapturedRecording {
    /// Frames dropped so far. `streaming_frames_dropped` only settles in
    /// `finalize_capture`, so any decision taken before finalize (preview paste
    /// runs before it, on purpose) must read the live counter or it always sees
    /// zero.
    ///
    /// This is a **lower bound** before finalize: capture-side drops are settled
    /// once `stop` has dropped the sender, but the worker can still add backlog
    /// drops while it drains its queue. Tail loss — the case that actually
    /// truncates a hypothesis — is reported separately and immediately by
    /// [`Self::streaming_delivery_failed`], which does not depend on this count.
    pub fn observed_frames_dropped(&self) -> u64 {
        match self.streaming_dropped.as_ref() {
            Some(dropped) => dropped.load(Ordering::Relaxed),
            None => self.streaming_frames_dropped,
        }
    }

    /// True once a frame has failed to reach the ASR host. The writer publishes
    /// this at the instant of failure, before it drains and counts the frames
    /// that died with it, so a decision taken without joining the worker still
    /// sees it. Everything after that frame is missing from the hypothesis.
    pub fn streaming_delivery_failed(&self) -> bool {
        self.streaming_send_failed
            .as_ref()
            .is_some_and(|failed| failed.load(Ordering::Relaxed))
    }

    /// True when the audio device, bounded capture queue, or durable writer
    /// reported a failure. The flag is sticky so callers can reject an
    /// incomplete fallback before the worker has finished draining its queue.
    pub fn streaming_write_failed(&self) -> bool {
        self.streaming_write_failed
            .as_ref()
            .is_some_and(|failed| failed.load(Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    pub active_speech_ms: u64,
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub noise_floor_dbfs: f32,
    pub snr_db: f32,
    pub clipping_ratio: f32,
}

impl RecorderService {
    pub fn new(recordings_dir: PathBuf) -> Self {
        Self {
            recordings_dir,
            active: Mutex::new(None),
        }
    }

    /// List durable dictation journals left by a process interruption. Meeting and
    /// microphone-check captures call `start` without streaming and therefore
    /// never create these files.
    pub fn list_recoverable_recordings(&self) -> Result<Vec<RecoverableRecording>> {
        list_recoverable_recordings(&self.recordings_dir)
    }

    /// Materialize one interrupted dictation into the normal WAV path while
    /// retaining its journal until History acknowledges the save.
    pub fn recover_recording(&self, id: &str) -> Result<FinishedRecording> {
        recover_recording(&self.recordings_dir, id)
    }

    /// Remove the recovery marker after the caller has durably saved the
    /// materialized recording in History. The WAV remains the source of truth.
    pub fn acknowledge_recovered_recording(&self, id: &str) -> Result<()> {
        acknowledge_recovered_recording(&self.recordings_dir, id)
    }

    pub fn list_microphones(&self, selected: Option<&str>) -> Result<Vec<MicrophoneInfo>> {
        if harness_audio_fixture().is_some() {
            return Ok(vec![MicrophoneInfo {
                name: harness_microphone_name().to_string(),
                is_default: true,
                is_selected: selected == Some(harness_microphone_name()),
                available: true,
            }]);
        }

        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|device| device.name().ok());
        let mut microphones = Vec::new();

        for device in host
            .input_devices()
            .context("failed to enumerate input devices")?
        {
            let name = device
                .name()
                .unwrap_or_else(|_| "Unknown microphone".to_string());
            microphones.push(MicrophoneInfo {
                is_default: default_name.as_ref() == Some(&name),
                is_selected: selected == Some(name.as_str()),
                available: true,
                name,
            });
        }

        Ok(microphones)
    }

    /// Start a raw recorder for microphone checks and Meeting Studio.
    /// `start(None)` intentionally does not create dictation checkpoints.
    pub fn start(
        &self,
        preferred_microphone: Option<String>,
        streaming_start: Option<StreamingStart>,
    ) -> Result<RecordingStarted> {
        self.start_mode(preferred_microphone, streaming_start, false)
    }

    /// Start dictation capture with durable audio enabled before ASR
    /// availability is considered. A missing or failing streaming host still
    /// leaves a bounded, recoverable capture for local fallback.
    pub fn start_dictation(
        &self,
        preferred_microphone: Option<String>,
        streaming_start: Option<StreamingStart>,
    ) -> Result<RecordingStarted> {
        self.start_mode(preferred_microphone, streaming_start, true)
    }

    fn start_mode(
        &self,
        preferred_microphone: Option<String>,
        streaming_start: Option<StreamingStart>,
        durable_dictation: bool,
    ) -> Result<RecordingStarted> {
        let mut active = self.active.lock();
        if active.is_some() {
            return Err(anyhow!("recording is already active"));
        }

        std::fs::create_dir_all(&self.recordings_dir)
            .context("failed to create recordings directory")?;

        if let Some(path) = harness_audio_fixture() {
            let (sample_rate, samples, duration_ms) = load_fixture_samples(&path)?;
            let id = Uuid::new_v4().to_string();
            let started_at = Utc::now();
            let recording_path = self.recordings_dir.join(format!("{id}.wav"));
            let samples_arc = Arc::new(Mutex::new(VecDeque::<f32>::new()));
            let streaming_host = if let Some(start) = streaming_start {
                match start
                    .host
                    .start_session(id.clone(), start.prompt, start.profile)
                {
                    Ok(()) => Some(start.host),
                    Err(error) => {
                        eprintln!(
                            "fixture streaming session unavailable, retaining durable capture: {error}"
                        );
                        None
                    }
                }
            } else {
                None
            };
            let streaming = if durable_dictation || streaming_host.is_some() {
                match start_streaming_worker(
                    streaming_host.clone(),
                    id.clone(),
                    samples_arc.clone(),
                    recording_path.clone(),
                ) {
                    Ok(streaming) => Some(streaming),
                    Err(error) => {
                        if let Some(host) = streaming_host.as_ref() {
                            host.cancel_session(&id);
                        }
                        if durable_dictation {
                            return Err(error);
                        }
                        eprintln!(
                            "fixture capture worker unavailable, retaining batch fallback: {error}"
                        );
                        None
                    }
                }
            } else {
                None
            };
            if let Some(streaming) = streaming.as_ref() {
                if let Some(sender) = streaming.sender.clone() {
                    let feed_rate = sample_rate;
                    let feed_samples = samples.clone();
                    let recent_samples = samples_arc.clone();
                    thread::Builder::new()
                        .name("atmospeak-fixture-feed".into())
                        .spawn(move || {
                            if let Err(error) = feed_fixture_through_sender(
                                &sender,
                                &recent_samples,
                                feed_rate,
                                &feed_samples,
                            ) {
                                eprintln!("fixture streaming feed failed: {error}");
                            }
                        })
                        .ok();
                }
            }
            if streaming.is_none() {
                // Preserve samples for batch fallback when streaming did not attach.
                *samples_arc.lock() = samples.into();
            }
            *active = Some(ActiveRecording {
                id: id.clone(),
                started_instant: Instant::now(),
                sample_rate,
                samples: samples_arc,
                duration_override_ms: Some(duration_ms),
                _stream: None,
                streaming,
            });
            return Ok(RecordingStarted {
                id,
                started_at,
                microphone_name: harness_microphone_name().to_string(),
            });
        }

        let host = cpal::default_host();
        let device = match preferred_microphone {
            Some(name) if !name.trim().is_empty() => host
                .input_devices()
                .context("failed to enumerate input devices")?
                .find(|device| device.name().ok().as_deref() == Some(name.as_str()))
                .ok_or_else(|| anyhow!("configured microphone not found: {name}"))?,
            _ => host
                .default_input_device()
                .ok_or_else(|| anyhow!("no default microphone is available"))?,
        };

        let microphone_name = device
            .name()
            .unwrap_or_else(|_| "Unknown microphone".to_string());
        let supported_config = device
            .default_input_config()
            .context("failed to get default input config")?;
        let sample_rate = supported_config.sample_rate().0;
        let channels = supported_config.channels() as usize;
        let stream_config = supported_config.clone().into();
        let id = Uuid::new_v4().to_string();
        let recording_path = self.recordings_dir.join(format!("{id}.wav"));
        let samples = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let streaming_host = if let Some(start) = streaming_start {
            match start
                .host
                .start_session(id.clone(), start.prompt, start.profile)
            {
                Ok(()) => Some(start.host),
                Err(error) => {
                    eprintln!(
                        "streaming ASR session unavailable, continuing with durable capture: {error}"
                    );
                    None
                }
            }
        } else {
            None
        };
        let mut streaming = if durable_dictation || streaming_host.is_some() {
            match start_streaming_worker(
                streaming_host.clone(),
                id.clone(),
                samples.clone(),
                recording_path.clone(),
            ) {
                Ok(streaming) => Some(streaming),
                Err(error) => {
                    if let Some(host) = streaming_host.as_ref() {
                        host.cancel_session(&id);
                    }
                    if durable_dictation {
                        return Err(error);
                    }
                    eprintln!("streaming worker unavailable, retaining batch fallback: {error}");
                    None
                }
            }
        } else {
            None
        };
        let stream_sender = streaming
            .as_mut()
            .and_then(|streaming| streaming.sender.take());
        let capture_samples = Arc::clone(&samples);
        let stream_write_failed_f32 = streaming
            .as_ref()
            .map(|streaming| streaming.write_failed.clone());
        let stream_write_failed_i16 = stream_write_failed_f32.clone();
        let stream_write_failed_u16 = stream_write_failed_f32.clone();
        let error_callback_f32 = move |error| {
            if let Some(failed) = stream_write_failed_f32.as_ref() {
                failed.store(true, Ordering::Relaxed);
            }
            eprintln!("Atmospeak audio stream error: {error}");
        };
        let error_callback_i16 = move |error| {
            if let Some(failed) = stream_write_failed_i16.as_ref() {
                failed.store(true, Ordering::Relaxed);
            }
            eprintln!("Atmospeak audio stream error: {error}");
        };
        let error_callback_u16 = move |error| {
            if let Some(failed) = stream_write_failed_u16.as_ref() {
                failed.store(true, Ordering::Relaxed);
            }
            eprintln!("Atmospeak audio stream error: {error}");
        };
        let stream_sender_f32 = stream_sender.clone();
        let stream_sender_i16 = stream_sender.clone();
        let stream_sender_u16 = stream_sender;

        let stream_result = match supported_config.sample_format() {
            SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        capture_input_streaming(
                            data,
                            channels,
                            sample_rate,
                            &capture_samples,
                            stream_sender_f32.as_ref(),
                        )
                    },
                    error_callback_f32,
                    None,
                )
                .map_err(anyhow::Error::from),
            SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        capture_input_streaming(
                            data,
                            channels,
                            sample_rate,
                            &capture_samples,
                            stream_sender_i16.as_ref(),
                        )
                    },
                    error_callback_i16,
                    None,
                )
                .map_err(anyhow::Error::from),
            SampleFormat::U16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        capture_input_streaming(
                            data,
                            channels,
                            sample_rate,
                            &capture_samples,
                            stream_sender_u16.as_ref(),
                        )
                    },
                    error_callback_u16,
                    None,
                )
                .map_err(anyhow::Error::from),
            format => Err(anyhow!("unsupported microphone sample format: {format:?}")),
        };
        let stream = match stream_result {
            Ok(stream) => stream,
            Err(error) => {
                abort_streaming_start(&mut streaming, &id, &recording_path);
                return Err(error.into());
            }
        };

        if let Err(error) = stream.play().context("failed to start microphone stream") {
            drop(stream);
            abort_streaming_start(&mut streaming, &id, &recording_path);
            return Err(error);
        }

        let started_at = Utc::now();
        *active = Some(ActiveRecording {
            id: id.clone(),
            started_instant: Instant::now(),
            sample_rate,
            samples,
            duration_override_ms: None,
            _stream: Some(stream),
            streaming,
        });

        Ok(RecordingStarted {
            id,
            started_at,
            microphone_name,
        })
    }

    pub fn stop(&self) -> Result<CapturedRecording> {
        let active = self
            .active
            .lock()
            .take()
            .ok_or_else(|| anyhow!("no active recording to stop"))?;
        let duration_ms = active
            .duration_override_ms
            .unwrap_or_else(|| active.started_instant.elapsed().as_millis() as u64);
        if Duration::from_millis(duration_ms) < Duration::from_millis(250) {
            cleanup_abandoned_recording(active, &self.recordings_dir);
            return Err(anyhow!("recording was too short to transcribe"));
        }

        let ActiveRecording {
            id,
            sample_rate,
            samples,
            duration_override_ms: _,
            _stream,
            mut streaming,
            ..
        } = active;
        drop(_stream);
        let (
            streaming_host,
            streaming_worker,
            streaming_worker_done,
            streaming_stop_requested,
            streaming_dropped,
            streaming_send_failed,
            streaming_write_failed,
            streaming_wav_ready,
            pending_samples,
            samples,
        ) = if let Some(mut streaming) = streaming.take() {
            (
                streaming.host,
                streaming.worker.take(),
                Some(streaming.worker_done),
                Some(streaming.stop_requested),
                Some(streaming.dropped),
                Some(streaming.send_failed),
                Some(streaming.write_failed),
                Some(streaming.wav_ready),
                Some(samples),
                Vec::new(),
            )
        } else {
            let samples = match Arc::try_unwrap(samples) {
                Ok(samples) => samples.into_inner().into_iter().collect(),
                Err(samples) => samples.lock().iter().copied().collect(),
            };
            (
                None, None, None, None, None, None, None, None, None, samples,
            )
        };
        if samples.is_empty() && pending_samples.is_none() {
            return Err(anyhow!("microphone did not capture any samples"));
        }

        Ok(CapturedRecording {
            path: self.recordings_dir.join(format!("{id}.wav")),
            id,
            duration_ms,
            sample_rate,
            samples,
            streaming_host,
            streaming_frames_dropped: 0,
            pending_samples,
            streaming_worker,
            streaming_worker_done,
            streaming_stop_requested,
            streaming_dropped,
            streaming_send_failed,
            streaming_write_failed,
            streaming_wav_ready,
            audio_prepared: false,
        })
    }

    pub fn cancel(&self) -> Result<()> {
        let mut active = self.active.lock();
        if let Some(mut active) = active.take() {
            drop(active._stream.take());
            if let Some(mut streaming) = active.streaming.take() {
                let recording_path = self.recordings_dir.join(format!("{}.wav", active.id));
                let _ = write_discard_marker(&recording_path, &active.id);
                stop_active_streaming(&mut streaming, &active.id);
                if fs::remove_file(journal_path_for_recording(&recording_path)).is_ok() {
                    clear_discard_marker(&recording_path);
                }
            }
            Ok(())
        } else {
            Err(anyhow!("no active recording to cancel"))
        }
    }

    pub fn mic_level(&self) -> MicLevel {
        let active = self.active.lock();
        let Some(active) = active.as_ref() else {
            return MicLevel {
                rms_dbfs: -96.0,
                peak_dbfs: -96.0,
                noise_floor_dbfs: -96.0,
                clipping_ratio: 0.0,
                timestamp_ms: Utc::now().timestamp_millis(),
            };
        };
        let samples = active.samples.lock();
        let sample_count = ((active.sample_rate as usize) / 2).clamp(512, 24_000);
        let start = samples.len().saturating_sub(sample_count);
        let window = samples.iter().skip(start).copied().collect::<Vec<_>>();
        if window.is_empty() {
            return MicLevel {
                rms_dbfs: -96.0,
                peak_dbfs: -96.0,
                noise_floor_dbfs: -96.0,
                clipping_ratio: 0.0,
                timestamp_ms: Utc::now().timestamp_millis(),
            };
        }
        let analysis = analyze_samples(&window, active.sample_rate);
        MicLevel {
            rms_dbfs: analysis.rms_dbfs,
            peak_dbfs: analysis.peak_dbfs,
            noise_floor_dbfs: analysis.noise_floor_dbfs,
            clipping_ratio: analysis.clipping_ratio,
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }
}

fn join_worker_bounded(
    worker: thread::JoinHandle<()>,
    worker_done: Option<&Arc<AtomicBool>>,
    stop_requested: Option<&Arc<AtomicBool>>,
) -> bool {
    let Some(worker_done) = worker_done else {
        drop(worker);
        return false;
    };
    let deadline = Instant::now() + STREAMING_WORKER_STOP_TIMEOUT;
    while !worker_done.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if worker_done.load(Ordering::Acquire) {
        return worker.join().is_ok();
    }
    if let Some(stop_requested) = stop_requested {
        stop_requested.store(true, Ordering::Release);
    }
    drop(worker);
    false
}

fn stop_active_streaming(streaming: &mut ActiveStreaming, session_id: &str) {
    // Cancellation is an intentional discard. Signal it before closing the
    // queue so the worker skips final journal publication and cannot resurrect
    // a user-cancelled capture during the next startup.
    streaming.discard_requested.store(true, Ordering::Release);
    streaming.stop_requested.store(true, Ordering::Release);
    drop(streaming.sender.take());
    if let Some(host) = streaming.host.as_ref() {
        host.cancel_session(session_id);
    }
    if let Some(worker) = streaming.worker.take() {
        if !join_worker_bounded(
            worker,
            Some(&streaming.worker_done),
            Some(&streaming.stop_requested),
        ) {
            streaming.write_failed.store(true, Ordering::Release);
        }
    }
}

fn abort_streaming_start(
    streaming: &mut Option<ActiveStreaming>,
    session_id: &str,
    recording_path: &std::path::Path,
) {
    if let Some(mut streaming) = streaming.take() {
        let _ = write_discard_marker(recording_path, session_id);
        stop_active_streaming(&mut streaming, session_id);
        if fs::remove_file(journal_path_for_recording(recording_path)).is_ok() {
            clear_discard_marker(recording_path);
        }
    }
    // Retain the WAV source, but remove the marker because this start failure
    // was handled synchronously and must not resurrect a discarded capture.
}

fn cleanup_abandoned_recording(mut active: ActiveRecording, recordings_dir: &std::path::Path) {
    drop(active._stream.take());
    if let Some(mut streaming) = active.streaming.take() {
        let recording_path = recordings_dir.join(format!("{}.wav", active.id));
        let _ = write_discard_marker(&recording_path, &active.id);
        stop_active_streaming(&mut streaming, &active.id);
        if fs::remove_file(journal_path_for_recording(&recording_path)).is_ok() {
            clear_discard_marker(&recording_path);
        }
    }
    // Retain the WAV source while removing the marker for this intentional
    // short-capture cleanup, so startup cannot resurrect it.
}

/// Complete recorder-worker draining after the capture edge has already been
/// acknowledged. This keeps shortcut latency independent of sidecar backlog.
pub fn finalize_capture(captured: &mut CapturedRecording) -> Result<()> {
    if let Some(worker) = captured.streaming_worker.take() {
        if !join_worker_bounded(
            worker,
            captured.streaming_worker_done.as_ref(),
            captured.streaming_stop_requested.as_ref(),
        ) {
            if let Some(write_failed) = captured.streaming_write_failed.as_ref() {
                write_failed.store(true, Ordering::Release);
            }
            return Err(anyhow!(
                "audio worker did not stop within {:?}; captured files were retained",
                STREAMING_WORKER_STOP_TIMEOUT
            ));
        }
    }
    if let Some(dropped) = captured.streaming_dropped.take() {
        captured.streaming_frames_dropped = dropped.load(Ordering::Relaxed);
    }
    if let Some(samples) = captured.pending_samples.take() {
        captured.samples = match Arc::try_unwrap(samples) {
            Ok(samples) => samples.into_inner().into_iter().collect(),
            Err(samples) => samples.lock().iter().copied().collect(),
        };
    }
    if let Some(wav_ready) = captured.streaming_wav_ready.as_ref() {
        // The streaming writer publishes this only after WavWriter::finalize
        // succeeds. A journal prefix remains available when finalization saw a
        // device or storage failure.
        captured.audio_prepared = wav_ready.load(Ordering::Acquire);
    }
    if captured.samples.is_empty() {
        Err(anyhow!("microphone did not capture any samples"))
    } else {
        Ok(())
    }
}

const HARNESS_MICROPHONE_NAME: &str = "Atmospeak Test Audio Fixture";

fn harness_microphone_name() -> &'static str {
    HARNESS_MICROPHONE_NAME
}

fn feed_fixture_through_sender(
    sender: &StreamingFrameSender,
    recent_samples: &Arc<Mutex<VecDeque<f32>>>,
    sample_rate: u32,
    samples: &[f32],
) -> Result<()> {
    // Pace fixture frames at the live ~20 ms cadence so the sidecar does not
    // accumulate a multi-second inference backlog before StopSession.
    let frame_samples = (sample_rate as usize / 50).max(1);
    let started = Instant::now();
    for (index, chunk) in samples.chunks(frame_samples).enumerate() {
        let scheduled =
            Duration::from_secs_f64(index as f64 * frame_samples as f64 / f64::from(sample_rate));
        if let Some(wait) = scheduled.checked_sub(started.elapsed()) {
            thread::sleep(wait);
        }
        let frame = InputFrame {
            sample_rate,
            samples: chunk.to_vec(),
        };
        append_recent_samples(recent_samples, &frame.samples, sample_rate);
        match sender.tx.try_send(frame) {
            Ok(()) => {}
            Err(TrySendError::Full(frame)) => {
                sender.capture_failed.store(true, Ordering::Release);
                sender.dropped.fetch_add(1, Ordering::Relaxed);
                sender
                    .tx
                    .send(frame)
                    .map_err(|_| anyhow!("fixture streaming worker exited while feeding audio"))?;
            }
            Err(TrySendError::Disconnected(_)) => {
                sender.capture_failed.store(true, Ordering::Release);
                sender.dropped.fetch_add(1, Ordering::Relaxed);
                return Err(anyhow!(
                    "fixture streaming worker exited while feeding audio"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn harness_audio_fixture() -> Option<PathBuf> {
    if std::env::var("ATMOSPEAK_NATIVE_HARNESS").ok().as_deref() != Some("1") {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("ATMOSPEAK_TEST_AUDIO_FIXTURE")?);
    path.is_file().then_some(path)
}

#[cfg(not(debug_assertions))]
fn harness_audio_fixture() -> Option<PathBuf> {
    None
}

fn load_fixture_samples(path: &std::path::Path) -> Result<(u32, Vec<f32>, u64)> {
    let mut reader = WavReader::open(path).with_context(|| {
        format!(
            "failed to open native harness audio fixture {}",
            path.display()
        )
    })?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 || spec.sample_rate == 0 {
        return Err(anyhow!(
            "native harness audio fixture has an invalid format"
        ));
    }

    let interleaved = match spec.sample_format {
        WavSampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to decode float native harness audio fixture")?,
        WavSampleFormat::Int if spec.bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|sample| sample.map(|value| value as f32 / i16::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to decode 16-bit native harness audio fixture")?,
        WavSampleFormat::Int => {
            let maximum = ((1_i64 << (spec.bits_per_sample.saturating_sub(1) as u32)) - 1) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / maximum))
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to decode integer native harness audio fixture")?
        }
    };
    if interleaved.is_empty() {
        return Err(anyhow!("native harness audio fixture is empty"));
    }

    let samples = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    };
    let duration_ms = ((samples.len() as u128 * 1_000) / spec.sample_rate as u128)
        .try_into()
        .unwrap_or(u64::MAX);
    Ok((spec.sample_rate, samples, duration_ms))
}

pub fn analyze_samples(samples: &[f32], sample_rate: u32) -> AudioAnalysis {
    if samples.is_empty() || sample_rate == 0 {
        return AudioAnalysis {
            active_speech_ms: 0,
            rms_dbfs: -96.0,
            peak_dbfs: -96.0,
            noise_floor_dbfs: -96.0,
            snr_db: 0.0,
            clipping_ratio: 0.0,
        };
    }

    let mean = samples.iter().copied().sum::<f32>() / samples.len() as f32;
    let frame_len = ((sample_rate as usize * 20) / 1_000).max(1);
    let mut peak = 0.0_f32;
    let mut clipped = 0_usize;
    let mut total_energy = 0.0_f32;
    let frames: Vec<_> = samples
        .chunks(frame_len)
        .map(|frame| {
            let mut energy = 0.0_f32;
            let mut precise_energy = 0.0_f64;
            for sample in frame {
                let centered = *sample - mean;
                peak = peak.max(centered.abs());
                clipped += usize::from(centered.abs() >= 0.99);
                energy += centered * centered;
                total_energy += centered * centered;
                precise_energy += (centered as f64) * (centered as f64);
            }
            (
                (energy / frame.len() as f32).sqrt(),
                precise_energy,
                frame.len(),
            )
        })
        .collect();
    let clipping_ratio = clipped as f32 / samples.len() as f32;
    let mut frame_rms: Vec<_> = frames.iter().map(|frame| frame.0).collect();
    frame_rms.sort_by(|a, b| a.total_cmp(b));
    let quiet_count = (frame_rms.len() / 4).max(1);
    let noise_rms = median(&frame_rms[..quiet_count]).max(1.0e-6);
    let noise_floor_dbfs = amplitude_dbfs(noise_rms);
    let active_threshold_dbfs = (noise_floor_dbfs + 10.0).max(-50.0);
    let active_threshold = 10.0_f32.powf(active_threshold_dbfs / 20.0);

    let mut active_energy = 0.0_f64;
    let mut active_samples = 0_usize;
    let mut active_frames = 0_u64;
    for (frame_rms, energy, count) in frames {
        if frame_rms < active_threshold {
            continue;
        }
        active_frames += 1;
        active_samples += count;
        active_energy += energy;
    }
    let active_rms = if active_samples > 0 {
        (active_energy / active_samples as f64).sqrt() as f32
    } else {
        (total_energy / samples.len() as f32).sqrt()
    };
    let rms_dbfs = amplitude_dbfs(active_rms);

    AudioAnalysis {
        active_speech_ms: active_frames * 20,
        rms_dbfs,
        peak_dbfs: amplitude_dbfs(peak),
        noise_floor_dbfs,
        snr_db: (rms_dbfs - noise_floor_dbfs).max(0.0),
        clipping_ratio,
    }
}

pub fn dictation_quality_warning(analysis: &AudioAnalysis) -> Option<&'static str> {
    if analysis.active_speech_ms < 250 {
        Some("No clear speech was detected. Check the selected microphone and try again.")
    } else if analysis.rms_dbfs < -48.0 {
        Some("The microphone signal is too quiet. Move closer or increase its Windows input level.")
    } else if analysis.clipping_ratio > 0.001 || analysis.peak_dbfs >= -1.0 {
        Some("The microphone is clipping. Lower its Windows input level and try again.")
    } else if analysis.snr_db < 8.0 {
        Some(
            "Speech is being masked by background noise. Reduce the noise or choose another microphone.",
        )
    } else {
        None
    }
}

pub fn analyze_for_dictation(captured: &CapturedRecording) -> Result<AudioAnalysis> {
    if captured.samples.is_empty() {
        return Err(anyhow!("microphone did not capture any samples"));
    }
    Ok(analyze_samples(&captured.samples, captured.sample_rate))
}

fn amplitude_dbfs(amplitude: f32) -> f32 {
    if amplitude <= 1.0e-6 {
        -96.0
    } else {
        (20.0 * amplitude.log10()).clamp(-96.0, 6.0)
    }
}

fn median(sorted: &[f32]) -> f32 {
    match sorted.len() {
        0 => 0.0,
        len if len % 2 == 1 => sorted[len / 2],
        len => (sorted[len / 2 - 1] + sorted[len / 2]) / 2.0,
    }
}

/// Save the original capture before analysis, decoding, or injection can fail.
pub fn persist_capture_audio(captured: &mut CapturedRecording) -> Result<()> {
    if captured.audio_prepared {
        return Ok(());
    }

    let journal_path = journal_path_for_recording(&captured.path);
    let journal = read_journal(&journal_path);
    if captured.streaming_wav_ready.is_some() {
        let Some(journal) = journal else {
            return Err(anyhow!(
                "durable capture journal is unavailable; captured files were retained"
            ));
        };
        let wav_samples = wav_data_sample_count(&captured.path).unwrap_or(0);
        let durable_samples = wav_samples.min(journal.sample_count);
        if durable_samples == 0 {
            return Err(anyhow!(
                "durable capture WAV has no committed samples; captured files were retained"
            ));
        }
        if journal.state == "active" {
            // The journal is the commit point for the WAV prefix. Repair the
            // header and truncate any bytes written after its last sync.
            recover_wav_from_partial(&captured.path, &captured.path, durable_samples)?;
        } else if wav_samples == 0 {
            return Err(anyhow!(
                "materialized capture WAV is invalid; captured files were retained"
            ));
        }
        captured.audio_prepared = true;
        return Ok(());
    }

    let resampled = resample_linear(&captured.samples, captured.sample_rate, TARGET_SAMPLE_RATE);
    if resampled.is_empty() {
        return Err(anyhow!("microphone did not capture any samples"));
    }
    write_wav(&captured.path, &resampled).context("failed to write recording wav")?;
    captured.audio_prepared = true;
    Ok(())
}

pub fn finish_recording(mut captured: CapturedRecording) -> Result<FinishedRecording> {
    persist_capture_audio(&mut captured)?;
    let journal_path = journal_path_for_recording(&captured.path);
    // A clean finalized source no longer needs a startup marker. Retain the
    // marker whenever any capture loss/write error was observed so History or
    // startup can salvage and diagnose the durable prefix.
    if !captured.streaming_write_failed() && checkpoint_journal_complete(&journal_path) {
        let _ = fs::remove_file(journal_path);
    }

    Ok(FinishedRecording {
        id: captured.id,
        path: captured.path,
        duration_ms: captured.duration_ms,
    })
}

trait IntoF32 {
    fn into_f32(self) -> f32;
}

impl IntoF32 for f32 {
    fn into_f32(self) -> f32 {
        self
    }
}

impl IntoF32 for i16 {
    fn into_f32(self) -> f32 {
        self as f32 / i16::MAX as f32
    }
}

impl IntoF32 for u16 {
    fn into_f32(self) -> f32 {
        (self as f32 - 32_768.0) / 32_768.0
    }
}

struct DurableWavFile(File);

impl Write for DurableWavFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()?;
        self.0.sync_data()
    }
}

impl Seek for DurableWavFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

fn create_durable_wav_writer(
    recording_path: &Path,
) -> Result<WavWriter<BufWriter<DurableWavFile>>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(recording_path)
        .with_context(|| {
            format!(
                "failed to create recording WAV {}",
                recording_path.display()
            )
        })?;
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };
    WavWriter::new(BufWriter::new(DurableWavFile(file)), spec)
        .map_err(anyhow::Error::from)
        .with_context(|| {
            format!(
                "failed to initialize recording WAV {}",
                recording_path.display()
            )
        })
}

fn start_streaming_worker(
    host: Option<Arc<StreamingAsr>>,
    session_id: String,
    _captured_samples: Arc<Mutex<VecDeque<f32>>>,
    recording_path: PathBuf,
) -> Result<ActiveStreaming> {
    // Open both durable files before starting the capture edge. A dictation
    // start either has a usable journal/WAV source or returns an error; it
    // never silently degrades into an unbounded in-memory fallback.
    let wav_writer = create_durable_wav_writer(&recording_path)?;
    let journal_path = journal_path_for_recording(&recording_path);
    let journal = CaptureJournal::create(&journal_path, &session_id)?;

    let (tx, rx) = mpsc::sync_channel::<InputFrame>(200);
    let dropped = Arc::new(AtomicU64::new(0));
    let send_failed = Arc::new(AtomicBool::new(false));
    let write_failed = Arc::new(AtomicBool::new(false));
    let wav_ready = Arc::new(AtomicBool::new(false));
    let worker_done = Arc::new(AtomicBool::new(false));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let discard_requested = Arc::new(AtomicBool::new(false));

    let worker_host = host.clone();
    let worker_dropped = dropped.clone();
    let worker_send_failed = send_failed.clone();
    let worker_write_failed = write_failed.clone();
    let worker_wav_ready = wav_ready.clone();
    let worker_done_flag = worker_done.clone();
    let worker_stop_requested = stop_requested.clone();
    let worker_discard_requested = discard_requested.clone();
    let writer_spawned = thread::Builder::new()
        .name("atmospeak-audio-stream".to_string())
        .spawn(move || {
            let (asr_tx, asr_rx) = mpsc::sync_channel::<(u64, Vec<u8>)>(100);
            let writer_done = Arc::new(AtomicBool::new(false));
            let writer_done_flag = writer_done.clone();
            let writer_stop_requested = worker_stop_requested.clone();
            let writer_host = worker_host.clone();
            let writer_session_id = session_id.clone();
            let writer_dropped = worker_dropped.clone();
            let writer_send_failed = worker_send_failed.clone();
            let writer = thread::Builder::new()
                .name("atmospeak-asr-writer".to_string())
                .spawn(move || {
                    let mut send_failed = false;
                    loop {
                        match asr_rx.recv_timeout(Duration::from_millis(100)) {
                            Ok((timestamp_ms, pcm)) => {
                                if writer_stop_requested.load(Ordering::Acquire) {
                                    break;
                                }
                                let Some(host) = writer_host.as_ref() else {
                                    continue;
                                };
                                if host
                                    .send_audio(&writer_session_id, timestamp_ms, pcm)
                                    .is_err()
                                {
                                    // Publish before draining so finality checks see the failure
                                    // even before this writer is joined.
                                    writer_send_failed.store(true, Ordering::Release);
                                    send_failed = true;
                                    break;
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                if writer_stop_requested.load(Ordering::Acquire) {
                                    break;
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    if send_failed {
                        // The failed frame plus everything still queued never
                        // reached the host. Count them so the engine falls
                        // back to batch instead of pasting a truncated tail.
                        let mut lost = 1_u64;
                        while asr_rx.try_recv().is_ok() {
                            lost += 1;
                        }
                        writer_dropped.fetch_add(lost, Ordering::Relaxed);
                    }
                    writer_done_flag.store(true, Ordering::Release);
                })
                .ok();
            if writer.is_none() && worker_host.is_some() {
                worker_send_failed.store(true, Ordering::Release);
            }

            let mut resampler = StreamingResampler::default();
            let mut wav_writer = Some(wav_writer);
            let mut journal = Some(journal);
            let mut durable_healthy = true;
            let mut durable_samples = 0_u64;
            let mut checkpoint_last_flush = Instant::now();
            let mut pending = Vec::<f32>::new();
            let mut timestamp_ms = 0_u64;

            loop {
                let frame = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(frame) => frame,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if worker_stop_requested.load(Ordering::Acquire) {
                            break;
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if worker_stop_requested.load(Ordering::Acquire) {
                    break;
                }
                let resampled = resampler.push(&frame.samples, frame.sample_rate);
                if let Some(writer) = wav_writer.as_mut() {
                    let mut frame_written = 0_u64;
                    for sample in &resampled {
                        match writer
                            .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        {
                            Ok(()) => frame_written = frame_written.saturating_add(1),
                            Err(_) => {
                                worker_write_failed.store(true, Ordering::Release);
                                durable_healthy = false;
                                wav_writer = None;
                                break;
                            }
                        }
                    }
                    durable_samples = durable_samples.saturating_add(frame_written);
                }
                // Feed recognition before periodic storage sync so its
                // existing writer can work while the durable checkpoint commits.
                pending.extend(resampled);
                while pending.len() >= 320 {
                    let pcm = encode_pcm_s16(&pending[..320]);
                    pending.drain(..320);
                    if asr_tx.try_send((timestamp_ms, pcm)).is_err() {
                        worker_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    timestamp_ms += 20;
                }
                if checkpoint_last_flush.elapsed() >= CHECKPOINT_FLUSH_INTERVAL {
                    let wav_flush = wav_writer.as_mut().map(|writer| writer.flush()).transpose();
                    if wav_flush.is_err() {
                        worker_write_failed.store(true, Ordering::Release);
                        durable_healthy = false;
                        wav_writer = None;
                    }
                    if matches!(wav_flush, Ok(Some(()))) {
                        let journal_result = journal
                            .as_mut()
                            .map(|journal| journal.record(durable_samples, false))
                            .transpose();
                        if journal_result.is_err() {
                            worker_write_failed.store(true, Ordering::Release);
                            durable_healthy = false;
                            journal = None;
                        }
                    }
                    checkpoint_last_flush = Instant::now();
                }
            }

            if !worker_stop_requested.load(Ordering::Acquire) && !pending.is_empty() {
                pending.resize(320, 0.0);
                let pcm = encode_pcm_s16(&pending);
                if asr_tx.try_send((timestamp_ms, pcm)).is_err() {
                    worker_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            drop(asr_tx);
            if let Some(writer) = writer {
                if !join_worker_bounded(writer, Some(&writer_done), Some(&worker_stop_requested)) {
                    worker_send_failed.store(true, Ordering::Release);
                }
            }

            if worker_discard_requested.load(Ordering::Acquire) {
                if let Some(journal) = journal {
                    let _ = journal.discard();
                }
                drop(wav_writer);
                worker_done_flag.store(true, Ordering::Release);
                return;
            }

            // The ASR writer has drained. Its Stop barrier finishes accepted
            // frames while we complete the mandatory durable WAV/journal writes.
            if !worker_stop_requested.load(Ordering::Acquire)
                && !worker_send_failed.load(Ordering::Acquire)
            {
                if let Some(host) = worker_host.as_ref() {
                    if host.request_stop(&session_id).is_err() {
                        worker_send_failed.store(true, Ordering::Release);
                    }
                }
            }

            // Hound finalization writes the final header and flushes our
            // DurableWavFile (including sync_data). A preceding flush would
            // write and sync the same header/data a second time.
            let wav_finish_started = Instant::now();
            let mut wav_finalized = false;
            if let Some(writer) = wav_writer {
                match writer.finalize() {
                    Ok(()) => {
                        wav_finalized = true;
                        worker_wav_ready.store(true, Ordering::Release);
                    }
                    Err(_) => worker_write_failed.store(true, Ordering::Release),
                }
            } else {
                worker_write_failed.store(true, Ordering::Release);
            }
            let wav_finish_ms = wav_finish_started.elapsed().as_millis();
            let journal_finish_started = Instant::now();
            if durable_healthy && wav_finalized {
                if let Some(journal) = journal {
                    // Commit the final count only after the finalized WAV is
                    // synced. Torn/failed records leave the prior prefix intact.
                    if journal.complete(durable_samples).is_err() {
                        worker_write_failed.store(true, Ordering::Release);
                    }
                }
            }
            eprintln!(
                "atmospeak capture-finalize session={} wav_sync_ms={} journal_sync_ms={}",
                session_id,
                wav_finish_ms,
                journal_finish_started.elapsed().as_millis()
            );
            worker_done_flag.store(true, Ordering::Release);
        })
        .context("failed to start streaming audio worker")?;

    Ok(ActiveStreaming {
        host,
        sender: Some(StreamingFrameSender {
            tx,
            dropped: dropped.clone(),
            capture_failed: write_failed.clone(),
        }),
        worker: Some(writer_spawned),
        worker_done,
        stop_requested,
        discard_requested,
        dropped,
        send_failed,
        write_failed,
        wav_ready,
    })
}

fn append_recent_samples(samples: &Arc<Mutex<VecDeque<f32>>>, frame: &[f32], sample_rate: u32) {
    let mut recent = samples.lock();
    recent.extend(frame.iter().copied());
    let maximum = sample_rate as usize * 10;
    let excess = recent.len().saturating_sub(maximum);
    for _ in 0..excess {
        let _ = recent.pop_front();
    }
}

#[derive(Default)]
struct StreamingResampler {
    buffer: Vec<f32>,
    position: f64,
}

impl StreamingResampler {
    fn push(&mut self, samples: &[f32], source_rate: u32) -> Vec<f32> {
        if source_rate == TARGET_SAMPLE_RATE {
            return samples.to_vec();
        }
        if source_rate == 0 || samples.is_empty() {
            return Vec::new();
        }
        self.buffer.extend_from_slice(samples);
        let ratio = source_rate as f64 / TARGET_SAMPLE_RATE as f64;
        let mut output = Vec::new();
        while self.position + 1.0 < self.buffer.len() as f64 {
            let left = self.position.floor() as usize;
            let fraction = (self.position - left as f64) as f32;
            output.push(self.buffer[left] * (1.0 - fraction) + self.buffer[left + 1] * fraction);
            self.position += ratio;
        }
        // Preserve the final source sample so interpolation across CPAL callback
        // boundaries is identical to interpolation over one contiguous buffer.
        let consumed = (self.position.floor() as usize).min(self.buffer.len().saturating_sub(1));
        if consumed > 0 {
            self.buffer.drain(..consumed);
            self.position -= consumed as f64;
        }
        output
    }
}

fn capture_input_streaming<T: Copy + IntoF32>(
    data: &[T],
    channels: usize,
    sample_rate: u32,
    samples: &Arc<Mutex<VecDeque<f32>>>,
    streaming: Option<&StreamingFrameSender>,
) {
    let mono = data
        .chunks(channels.max(1))
        .map(|frame| {
            frame.iter().map(|sample| (*sample).into_f32()).sum::<f32>() / frame.len() as f32
        })
        .collect::<Vec<_>>();
    if let Some(streaming) = streaming {
        // Keep only a bounded tail for level/quality inspection. The complete
        // fallback source is the durable WAV journal, never this callback
        // ring, so long dictations cannot grow memory without bound.
        append_recent_samples(samples, &mono, sample_rate);
        match streaming.tx.try_send(InputFrame {
            sample_rate,
            samples: mono,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_frame)) | Err(TrySendError::Disconnected(_frame)) => {
                // The durable writer consumes this same bounded queue. A
                // callback drop therefore means the recoverable source has a
                // missing prefix and must never be presented as complete.
                streaming.capture_failed.store(true, Ordering::Release);
                streaming.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    } else {
        samples.lock().extend(mono);
    }
}

fn capture_input<T: Copy + IntoF32>(
    data: &[T],
    channels: usize,
    samples: &Arc<Mutex<VecDeque<f32>>>,
) {
    let mut output = samples.lock();
    for frame in data.chunks(channels.max(1)) {
        let sum = frame.iter().map(|sample| (*sample).into_f32()).sum::<f32>();
        output.push_back(sum / frame.len() as f32);
    }
}

fn resample_linear(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = source_rate as f64 / target_rate as f64;
    let target_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(target_len);

    for index in 0..target_len {
        let source_position = index as f64 * ratio;
        let left = source_position.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let fraction = (source_position - left as f64) as f32;
        let sample = samples[left] * (1.0 - fraction) + samples[right] * fraction;
        output.push(sample);
    }

    output
}

fn write_wav(path: &PathBuf, samples: &[f32]) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)?;

    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        writer.write_sample((clamped * i16::MAX as f32) as i16)?;
    }

    writer.finalize()?;
    Ok(())
}

fn encode_pcm_s16(samples: &[f32]) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len().saturating_mul(WAV_SAMPLE_BYTES as usize));
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    pcm
}

#[cfg(test)]
mod tests {
    use super::{
        AudioAnalysis, CaptureJournal, CapturedRecording, StreamingFrameSender, StreamingResampler,
        TARGET_SAMPLE_RATE, acknowledge_recovered_recording, analyze_for_dictation,
        analyze_samples, capture_input_streaming, dictation_quality_warning, finalize_capture,
        finish_recording, list_recoverable_recordings, load_fixture_samples, read_journal,
        recover_recording, resample_linear, start_streaming_worker,
    };
    use parking_lot::Mutex;
    use std::collections::VecDeque;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    };
    use tempfile::tempdir;

    #[test]
    fn interrupted_wav_journal_is_listed_recovered_and_acknowledged() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("interrupted.wav");
        let journal_path = super::journal_path_for_recording(&path);
        let samples = [0.1_f32, -0.2, 0.3, -0.4, 0.5, -0.6, 0.7, -0.8];
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: TARGET_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("partial wav");
        for sample in samples {
            writer
                .write_sample((sample * i16::MAX as f32) as i16)
                .expect("sample");
        }
        writer.finalize().expect("wav finalize");
        // Model a torn size-field update after an earlier journal commit.
        let mut header = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open header");
        std::io::Seek::seek(&mut header, std::io::SeekFrom::Start(40)).expect("seek size");
        std::io::Write::write_all(&mut header, &2_u32.to_le_bytes()).expect("torn size");
        header.sync_all().expect("sync header");
        drop(header);

        let mut journal = CaptureJournal::create(&journal_path, "interrupted").expect("journal");
        journal.record(4, true).expect("checkpoint count");

        drop(journal);
        let mut torn = std::fs::OpenOptions::new()
            .append(true)
            .open(&journal_path)
            .expect("reopen journal");
        std::io::Write::write_all(&mut torn, b"record=3|active|999|").expect("torn append");
        drop(torn);
        assert_eq!(
            read_journal(&journal_path)
                .expect("last valid journal")
                .sample_count,
            4
        );

        let recoverable = list_recoverable_recordings(temp.path()).expect("list recovery");
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].id, "interrupted");
        assert_eq!(recoverable[0].sample_count, 4);
        assert!(!recoverable[0].materialized);

        let finished = recover_recording(temp.path(), "interrupted").expect("recover");
        let mut reader = hound::WavReader::open(&finished.path).expect("recovered wav");
        assert_eq!(reader.spec().sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.samples::<i16>().count(), 4);
        assert!(journal_path.exists());

        let materialized = list_recoverable_recordings(temp.path()).expect("list materialized");
        assert!(materialized[0].materialized);
        acknowledge_recovered_recording(temp.path(), "interrupted").expect("acknowledge");
        assert!(!journal_path.exists());
        assert!(path.exists());
    }

    #[test]
    fn streaming_worker_persists_one_recoverable_wav_without_asr_host() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("worker.wav");
        let samples = Arc::new(Mutex::new(VecDeque::new()));
        let mut active = start_streaming_worker(None, "worker".to_string(), samples, path.clone())
            .expect("start durable worker");
        let sender = active.sender.take().expect("worker sender");
        sender
            .tx
            .send(super::InputFrame {
                sample_rate: TARGET_SAMPLE_RATE,
                samples: vec![0.25; 320],
            })
            .expect("send frame");
        drop(sender);
        active
            .worker
            .take()
            .expect("worker handle")
            .join()
            .expect("join worker");

        let mut reader = hound::WavReader::open(&path).expect("durable wav");
        assert_eq!(reader.spec().sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(reader.samples::<i16>().count(), 320);
        let journal = read_journal(&super::journal_path_for_recording(&path)).expect("journal");
        assert_eq!(journal.state, "complete");
        assert_eq!(journal.sample_count, 320);
        assert!(!path.with_extension("pcm").exists());
    }

    #[test]
    fn finish_recording_writes_mono_sixteen_kilohertz_wav() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("sample.wav");
        let samples = (0..48_000)
            .map(|index| ((index as f32 / 24.0).sin() * 0.2).clamp(-1.0, 1.0))
            .collect::<Vec<_>>();

        let finished = finish_recording(CapturedRecording {
            id: "sample".to_string(),
            path: path.clone(),
            duration_ms: 1_000,
            sample_rate: 48_000,
            samples,
            streaming_host: None,
            streaming_frames_dropped: 0,
            pending_samples: None,
            streaming_worker: None,
            streaming_worker_done: None,
            streaming_stop_requested: None,
            streaming_dropped: None,
            streaming_send_failed: None,
            streaming_write_failed: None,
            streaming_wav_ready: None,
            audio_prepared: false,
        })
        .expect("finish recording");

        let reader = hound::WavReader::open(&path).expect("wav reader");
        let spec = reader.spec();
        assert_eq!(finished.id, "sample");
        assert_eq!(finished.path, path);
        assert_eq!(finished.duration_ms, 1_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(spec.bits_per_sample, 16);
    }

    #[test]
    fn incremental_resampler_matches_offline_resampler_within_tolerance() {
        let samples = (0..48_000)
            .map(|index| (index as f32 / 19.0).sin() * 0.3)
            .collect::<Vec<_>>();
        let expected = resample_linear(&samples, 48_000, TARGET_SAMPLE_RATE);
        let mut resampler = StreamingResampler::default();
        let actual = samples
            .chunks(997)
            .flat_map(|chunk| resampler.push(chunk, 48_000))
            .collect::<Vec<_>>();
        assert!(expected.len().abs_diff(actual.len()) <= 1);
        for (expected, actual) in expected.iter().zip(&actual) {
            assert!((expected - actual).abs() < 0.0001);
        }
    }

    #[test]
    fn full_streaming_queue_never_blocks_the_audio_callback() {
        let (tx, _rx) = mpsc::sync_channel(0);
        let dropped = Arc::new(AtomicU64::new(0));
        let sender = StreamingFrameSender {
            tx,
            dropped: dropped.clone(),
            capture_failed: Arc::new(AtomicBool::new(false)),
        };
        let captured = Arc::new(Mutex::new(VecDeque::new()));
        capture_input_streaming(&[0.1_f32; 320], 1, 16_000, &captured, Some(&sender));
        assert_eq!(captured.lock().len(), 320);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn streaming_meter_buffer_stays_bounded_over_long_capture() {
        let (tx, _rx) = mpsc::sync_channel(200);
        let dropped = Arc::new(AtomicU64::new(0));
        let sender = StreamingFrameSender {
            tx,
            dropped,
            capture_failed: Arc::new(AtomicBool::new(false)),
        };
        let captured = Arc::new(Mutex::new(VecDeque::new()));
        let frame = [0.1_f32; 320];
        for _ in 0..((TARGET_SAMPLE_RATE as usize * 11) / frame.len()) {
            capture_input_streaming(&frame, 1, TARGET_SAMPLE_RATE, &captured, Some(&sender));
        }

        assert_eq!(captured.lock().len(), TARGET_SAMPLE_RATE as usize * 10);
    }

    #[test]
    fn native_harness_fixture_is_decoded_and_downmixed() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("fixture.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("wav writer");
        for _ in 0..16_000 {
            writer.write_sample(8_000_i16).expect("left");
            writer.write_sample(4_000_i16).expect("right");
        }
        writer.finalize().expect("finalize");

        let (sample_rate, samples, duration_ms) =
            load_fixture_samples(&path).expect("load fixture");
        assert_eq!(sample_rate, 16_000);
        assert_eq!(samples.len(), 16_000);
        assert_eq!(duration_ms, 1_000);
        assert!((samples[0] - (6_000.0 / i16::MAX as f32)).abs() < 0.0001);
    }

    #[test]
    fn quiet_capture_reports_a_quality_warning() {
        let samples = (0..48_000)
            .map(|index| (index as f32 / 18.0).sin() * 0.0008)
            .collect::<Vec<_>>();
        let analysis = analyze_samples(&samples, 48_000);
        assert!(analysis.rms_dbfs < -48.0);
        assert!(dictation_quality_warning(&analysis).is_some());
    }

    #[test]
    fn noisy_and_clipped_captures_report_quality_warnings() {
        let noisy = AudioAnalysis {
            active_speech_ms: 800,
            rms_dbfs: -24.0,
            peak_dbfs: -8.0,
            noise_floor_dbfs: -27.0,
            snr_db: 3.0,
            clipping_ratio: 0.0,
        };
        assert!(
            dictation_quality_warning(&noisy)
                .expect("noisy rejection")
                .contains("background noise")
        );

        let clipped = AudioAnalysis {
            active_speech_ms: 800,
            rms_dbfs: -12.0,
            peak_dbfs: -0.2,
            noise_floor_dbfs: -50.0,
            snr_db: 38.0,
            clipping_ratio: 0.01,
        };
        assert!(
            dictation_quality_warning(&clipped)
                .expect("clipping rejection")
                .contains("clipping")
        );
    }

    #[test]
    fn valid_speech_is_analyzed_without_mutating_recorded_samples() {
        let temp = tempdir().expect("tempdir");
        let mut samples = vec![0.0002; 24_000];
        samples.extend((0..72_000).map(|index| (index as f32 / 18.0).sin() * 0.08));
        let captured = CapturedRecording {
            id: "quality".to_string(),
            path: temp.path().join("quality.wav"),
            duration_ms: 2_000,
            sample_rate: 48_000,
            samples: samples.clone(),
            streaming_host: None,
            streaming_frames_dropped: 0,
            pending_samples: None,
            streaming_worker: None,
            streaming_worker_done: None,
            streaming_stop_requested: None,
            streaming_dropped: None,
            streaming_send_failed: None,
            streaming_write_failed: None,
            streaming_wav_ready: None,
            audio_prepared: true,
        };
        let analysis = analyze_for_dictation(&captured).expect("valid speech");
        assert!(analysis.active_speech_ms >= 250);
        assert_eq!(captured.samples, samples);
    }

    /// Preview paste decides before `finalize_capture`, so the drop gate must see
    /// the live counter instead of the not-yet-settled field.
    #[test]
    fn observed_drops_read_the_live_counter_before_finalize() {
        let temp = tempdir().expect("tempdir");
        let dropped = Arc::new(AtomicU64::new(31));
        let mut captured = CapturedRecording {
            id: "drops".to_string(),
            path: temp.path().join("drops.wav"),
            duration_ms: 2_000,
            sample_rate: TARGET_SAMPLE_RATE,
            samples: vec![0.0; 16],
            streaming_host: None,
            streaming_frames_dropped: 0,
            pending_samples: None,
            streaming_worker: None,
            streaming_worker_done: None,
            streaming_stop_requested: None,
            streaming_dropped: Some(dropped.clone()),
            streaming_send_failed: None,
            streaming_write_failed: None,
            streaming_wav_ready: None,
            audio_prepared: true,
        };
        assert_eq!(captured.observed_frames_dropped(), 31);
        assert!(!captured.streaming_delivery_failed());

        // After finalize the atomic is gone and the settled field answers instead.
        finalize_capture(&mut captured).expect("finalize");
        assert_eq!(captured.streaming_frames_dropped, 31);
        assert_eq!(captured.observed_frames_dropped(), 31);
    }

    /// Tail loss must be visible without joining the writer: the count that goes
    /// with it is only added once the writer drains, which is after preview paste
    /// has already decided.
    #[test]
    fn delivery_failure_is_visible_while_the_drop_count_is_still_zero() {
        let temp = tempdir().expect("tempdir");
        let send_failed = Arc::new(AtomicBool::new(false));
        let captured = CapturedRecording {
            id: "tail-loss".to_string(),
            path: temp.path().join("tail-loss.wav"),
            duration_ms: 2_000,
            sample_rate: TARGET_SAMPLE_RATE,
            samples: vec![0.0; 16],
            streaming_host: None,
            streaming_frames_dropped: 0,
            pending_samples: None,
            streaming_worker: None,
            streaming_worker_done: None,
            streaming_stop_requested: None,
            streaming_dropped: Some(Arc::new(AtomicU64::new(0))),
            streaming_send_failed: Some(send_failed.clone()),
            streaming_write_failed: None,
            streaming_wav_ready: None,
            audio_prepared: true,
        };
        assert!(!captured.streaming_delivery_failed());

        // What the writer thread does the instant `send_audio` fails.
        send_failed.store(true, Ordering::Relaxed);
        assert_eq!(captured.observed_frames_dropped(), 0);
        assert!(captured.streaming_delivery_failed());
    }
}
