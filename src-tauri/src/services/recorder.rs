use std::{
    path::PathBuf,
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

use crate::{
    models::{MicLevel, MicrophoneInfo, RecordingStarted, TranscriptionProfile},
    services::streaming_asr::StreamingAsr,
};

const TARGET_SAMPLE_RATE: u32 = 16_000;

pub struct RecorderService {
    recordings_dir: PathBuf,
    active: Mutex<Option<ActiveRecording>>,
}

struct ActiveRecording {
    id: String,
    started_instant: Instant,
    sample_rate: u32,
    samples: Arc<Mutex<Vec<f32>>>,
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
    host: Arc<StreamingAsr>,
    sender: Option<StreamingFrameSender>,
    worker: Option<thread::JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
    write_failed: Arc<AtomicBool>,
}

struct InputFrame {
    sample_rate: u32,
    samples: Vec<f32>,
}

#[derive(Clone)]
struct StreamingFrameSender {
    tx: SyncSender<InputFrame>,
    dropped: Arc<AtomicU64>,
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
    pending_samples: Option<Arc<Mutex<Vec<f32>>>>,
    streaming_worker: Option<thread::JoinHandle<()>>,
    streaming_dropped: Option<Arc<AtomicU64>>,
    streaming_write_failed: Option<Arc<AtomicBool>>,
    audio_prepared: bool,
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

    pub fn start(
        &self,
        preferred_microphone: Option<String>,
        streaming_start: Option<StreamingStart>,
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
            *active = Some(ActiveRecording {
                id: id.clone(),
                started_instant: Instant::now(),
                sample_rate,
                samples: Arc::new(Mutex::new(samples)),
                duration_override_ms: Some(duration_ms),
                _stream: None,
                streaming: None,
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
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let mut streaming = streaming_start.and_then(|start| {
            match start
                .host
                .start_session(id.clone(), start.prompt, start.profile)
            {
                Ok(()) => Some(start_streaming_worker(
                    start.host,
                    id.clone(),
                    samples.clone(),
                    recording_path.clone(),
                )),
                Err(error) => {
                    eprintln!(
                        "streaming ASR session unavailable, retaining batch fallback: {error}"
                    );
                    None
                }
            }
        });
        let stream_sender = streaming
            .as_mut()
            .and_then(|streaming| streaming.sender.take());
        let capture_samples = Arc::clone(&samples);
        let error_callback = |error| eprintln!("Atmospeak audio stream error: {error}");
        let stream_sender_f32 = stream_sender.clone();
        let stream_sender_i16 = stream_sender.clone();
        let stream_sender_u16 = stream_sender;

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
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
                error_callback,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
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
                error_callback,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
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
                error_callback,
                None,
            )?,
            format => return Err(anyhow!("unsupported microphone sample format: {format:?}")),
        };

        stream.play().context("failed to start microphone stream")?;

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
            streaming_dropped,
            streaming_write_failed,
            pending_samples,
            samples,
        ) = if let Some(mut streaming) = streaming.take() {
            (
                Some(streaming.host),
                streaming.worker.take(),
                Some(streaming.dropped),
                Some(streaming.write_failed),
                Some(samples),
                Vec::new(),
            )
        } else {
            let samples = match Arc::try_unwrap(samples) {
                Ok(samples) => samples.into_inner(),
                Err(samples) => samples.lock().clone(),
            };
            (None, None, None, None, None, samples)
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
            streaming_dropped,
            streaming_write_failed,
            audio_prepared: false,
        })
    }

    pub fn cancel(&self) -> Result<()> {
        let mut active = self.active.lock();
        if let Some(mut active) = active.take() {
            drop(active._stream.take());
            if let Some(mut streaming) = active.streaming.take() {
                if let Some(worker) = streaming.worker.take() {
                    let _ = worker.join();
                }
                streaming.host.cancel_session(&active.id);
                let _ = std::fs::remove_file(
                    self.recordings_dir.join(format!("{}.wav", active.id)),
                );
            }
            Ok(())
        } else {
            Err(anyhow!("no active recording to cancel"))
        }
    }

    pub fn level(&self) -> f32 {
        let active = self.active.lock();
        let Some(active) = active.as_ref() else {
            return 0.0;
        };
        let samples = active.samples.lock();
        if samples.is_empty() {
            return 0.0;
        }

        let sample_count = ((active.sample_rate as usize) / 10).clamp(256, 4_800);
        let start = samples.len().saturating_sub(sample_count);
        let window = &samples[start..];
        let rms =
            (window.iter().map(|sample| sample * sample).sum::<f32>() / window.len() as f32).sqrt();

        (rms * 4.0).clamp(0.0, 1.0)
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
        let window = &samples[start..];
        if window.is_empty() {
            return MicLevel {
                rms_dbfs: -96.0,
                peak_dbfs: -96.0,
                noise_floor_dbfs: -96.0,
                clipping_ratio: 0.0,
                timestamp_ms: Utc::now().timestamp_millis(),
            };
        }
        let analysis = analyze_samples(window, active.sample_rate);
        MicLevel {
            rms_dbfs: analysis.rms_dbfs,
            peak_dbfs: analysis.peak_dbfs,
            noise_floor_dbfs: analysis.noise_floor_dbfs,
            clipping_ratio: analysis.clipping_ratio,
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }
}

/// Complete recorder-worker draining after the capture edge has already been
/// acknowledged. This keeps shortcut latency independent of sidecar backlog.
pub fn finalize_capture(captured: &mut CapturedRecording) -> Result<()> {
    if let Some(worker) = captured.streaming_worker.take() {
        let _ = worker.join();
    }
    if let Some(dropped) = captured.streaming_dropped.take() {
        captured.streaming_frames_dropped = dropped.load(Ordering::Relaxed);
    }
    if let Some(write_failed) = captured.streaming_write_failed.take() {
        if write_failed.load(Ordering::Relaxed) {
            return Err(anyhow!("failed to persist the streaming audio capture"));
        }
        captured.audio_prepared = true;
    }
    if let Some(samples) = captured.pending_samples.take() {
        captured.samples = match Arc::try_unwrap(samples) {
            Ok(samples) => samples.into_inner(),
            Err(samples) => samples.lock().clone(),
        };
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
    let centered = samples
        .iter()
        .map(|sample| sample - mean)
        .collect::<Vec<_>>();
    let peak = centered
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    let clipping_ratio = centered
        .iter()
        .filter(|sample| sample.abs() >= 0.99)
        .count() as f32
        / centered.len() as f32;

    let frame_len = ((sample_rate as usize * 20) / 1_000).max(1);
    let mut frame_rms = centered
        .chunks(frame_len)
        .filter(|frame| !frame.is_empty())
        .map(rms)
        .collect::<Vec<_>>();
    frame_rms.sort_by(|a, b| a.total_cmp(b));
    let quiet_count = (frame_rms.len() / 4).max(1);
    let noise_rms = median(&frame_rms[..quiet_count]).max(1.0e-6);
    let noise_floor_dbfs = amplitude_dbfs(noise_rms);
    let active_threshold_dbfs = (noise_floor_dbfs + 10.0).max(-50.0);
    let active_threshold = 10.0_f32.powf(active_threshold_dbfs / 20.0);

    let mut active_energy = 0.0_f64;
    let mut active_samples = 0_usize;
    let mut active_frames = 0_u64;
    for frame in centered.chunks(frame_len) {
        if frame.is_empty() || rms(frame) < active_threshold {
            continue;
        }
        active_frames += 1;
        active_samples += frame.len();
        active_energy += frame
            .iter()
            .map(|sample| (*sample as f64) * (*sample as f64))
            .sum::<f64>();
    }
    let active_rms = if active_samples > 0 {
        (active_energy / active_samples as f64).sqrt() as f32
    } else {
        rms(&centered)
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

pub fn normal_dictation_failure(analysis: &AudioAnalysis) -> Option<&'static str> {
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

pub fn prepare_for_dictation(captured: &mut CapturedRecording) -> Result<AudioAnalysis> {
    let analysis = analyze_samples(&captured.samples, captured.sample_rate);
    if let Some(message) = normal_dictation_failure(&analysis) {
        return Err(anyhow!(message));
    }

    let mean = captured.samples.iter().copied().sum::<f32>() / captured.samples.len() as f32;
    for sample in &mut captured.samples {
        *sample -= mean;
    }
    let peak = captured
        .samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    if peak > 0.0 {
        let target_peak = 10.0_f32.powf(-3.0 / 20.0);
        let gain = (target_peak / peak).min(10.0_f32.powf(18.0 / 20.0));
        for sample in &mut captured.samples {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
    Ok(analysis)
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
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

pub fn finish_recording(captured: CapturedRecording) -> Result<FinishedRecording> {
    if !captured.audio_prepared {
        let resampled =
            resample_linear(&captured.samples, captured.sample_rate, TARGET_SAMPLE_RATE);
        write_wav(&captured.path, &resampled).context("failed to write recording wav")?;
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

fn start_streaming_worker(
    host: Arc<StreamingAsr>,
    session_id: String,
    captured_samples: Arc<Mutex<Vec<f32>>>,
    recording_path: PathBuf,
) -> ActiveStreaming {
    let (tx, rx) = mpsc::sync_channel::<InputFrame>(200);
    let dropped = Arc::new(AtomicU64::new(0));
    let worker_host = host.clone();
    let worker_dropped = dropped.clone();
    let write_failed = Arc::new(AtomicBool::new(false));
    let worker_write_failed = write_failed.clone();
    let worker = thread::Builder::new()
        .name("atmospeak-audio-stream".to_string())
        .spawn(move || {
            let (asr_tx, asr_rx) = mpsc::sync_channel::<(u64, Vec<u8>)>(100);
            let writer_host = worker_host.clone();
            let writer_session_id = session_id.clone();
            let writer = thread::Builder::new()
                .name("atmospeak-asr-writer".to_string())
                .spawn(move || {
                    while let Ok((timestamp_ms, pcm)) = asr_rx.recv() {
                        if writer_host
                            .send_audio(&writer_session_id, timestamp_ms, pcm)
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .ok();
            let mut resampler = StreamingResampler::default();
            let wav_spec = WavSpec {
                channels: 1,
                sample_rate: TARGET_SAMPLE_RATE,
                bits_per_sample: 16,
                sample_format: WavSampleFormat::Int,
            };
            let mut wav_writer = WavWriter::create(&recording_path, wav_spec)
                .map_err(|_| worker_write_failed.store(true, Ordering::Relaxed))
                .ok();
            let mut pending = Vec::<f32>::new();
            let mut timestamp_ms = 0_u64;
            while let Ok(frame) = rx.recv() {
                {
                    let mut recent = captured_samples.lock();
                    recent.extend_from_slice(&frame.samples);
                    let maximum = frame.sample_rate as usize * 10;
                    if recent.len() > maximum {
                        let excess = recent.len() - maximum;
                        recent.drain(..excess);
                    }
                }
                let resampled = resampler.push(&frame.samples, frame.sample_rate);
                if let Some(writer) = wav_writer.as_mut() {
                    for sample in &resampled {
                        if writer
                            .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                            .is_err()
                        {
                            worker_write_failed.store(true, Ordering::Relaxed);
                            wav_writer = None;
                            break;
                        }
                    }
                }
                pending.extend(resampled);
                while pending.len() >= 320 {
                    let pcm = pending
                        .drain(..320)
                        .flat_map(|sample| {
                            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            value.to_le_bytes()
                        })
                        .collect::<Vec<_>>();
                    if asr_tx.try_send((timestamp_ms, pcm)).is_err() {
                        worker_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    timestamp_ms += 20;
                }
            }
            if !pending.is_empty() {
                pending.resize(320, 0.0);
                let pcm = pending
                    .into_iter()
                    .flat_map(|sample| {
                        ((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes()
                    })
                    .collect::<Vec<_>>();
                if asr_tx.try_send((timestamp_ms, pcm)).is_err() {
                    worker_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            drop(asr_tx);
            if let Some(writer) = wav_writer
                && writer.finalize().is_err()
            {
                worker_write_failed.store(true, Ordering::Relaxed);
            }
            if let Some(writer) = writer {
                let _ = writer.join();
            }
        })
        .ok();
    ActiveStreaming {
        host,
        sender: Some(StreamingFrameSender {
            tx,
            dropped: dropped.clone(),
        }),
        worker,
        dropped,
        write_failed,
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
    samples: &Arc<Mutex<Vec<f32>>>,
    streaming: Option<&StreamingFrameSender>,
) {
    let mono = data
        .chunks(channels.max(1))
        .map(|frame| {
            frame.iter().map(|sample| (*sample).into_f32()).sum::<f32>() / frame.len() as f32
        })
        .collect::<Vec<_>>();
    if let Some(streaming) = streaming {
        match streaming.tx.try_send(InputFrame {
            sample_rate,
            samples: mono,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                streaming.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    } else {
        samples.lock().extend_from_slice(&mono);
    }
}

fn capture_input<T: Copy + IntoF32>(data: &[T], channels: usize, samples: &Arc<Mutex<Vec<f32>>>) {
    let mut output = samples.lock();
    for frame in data.chunks(channels.max(1)) {
        let sum = frame.iter().map(|sample| (*sample).into_f32()).sum::<f32>();
        output.push(sum / frame.len() as f32);
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

#[cfg(test)]
mod tests {
    use super::{
        AudioAnalysis, CapturedRecording, StreamingFrameSender, StreamingResampler,
        TARGET_SAMPLE_RATE, analyze_samples, capture_input_streaming, finish_recording,
        load_fixture_samples, normal_dictation_failure, prepare_for_dictation, resample_linear,
    };
    use parking_lot::Mutex;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    };
    use tempfile::tempdir;

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
            streaming_dropped: None,
            streaming_write_failed: None,
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
        };
        let captured = Arc::new(Mutex::new(Vec::new()));
        capture_input_streaming(&[0.1_f32; 320], 1, 16_000, &captured, Some(&sender));
        assert!(captured.lock().is_empty());
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
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
    fn quiet_capture_is_rejected_before_asr() {
        let samples = (0..48_000)
            .map(|index| (index as f32 / 18.0).sin() * 0.0008)
            .collect::<Vec<_>>();
        let analysis = analyze_samples(&samples, 48_000);
        assert!(analysis.rms_dbfs < -48.0);
        assert!(normal_dictation_failure(&analysis).is_some());
    }

    #[test]
    fn noisy_and_clipped_captures_are_rejected_before_asr() {
        let noisy = AudioAnalysis {
            active_speech_ms: 800,
            rms_dbfs: -24.0,
            peak_dbfs: -8.0,
            noise_floor_dbfs: -27.0,
            snr_db: 3.0,
            clipping_ratio: 0.0,
        };
        assert!(
            normal_dictation_failure(&noisy)
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
            normal_dictation_failure(&clipped)
                .expect("clipping rejection")
                .contains("clipping")
        );
    }

    #[test]
    fn valid_speech_is_dc_removed_and_normalized_with_a_gain_cap() {
        let temp = tempdir().expect("tempdir");
        let mut samples = vec![0.0002; 24_000];
        samples.extend((0..72_000).map(|index| (index as f32 / 18.0).sin() * 0.08));
        let mut captured = CapturedRecording {
            id: "quality".to_string(),
            path: temp.path().join("quality.wav"),
            duration_ms: 2_000,
            sample_rate: 48_000,
            samples,
            streaming_host: None,
            streaming_frames_dropped: 0,
            pending_samples: None,
            streaming_worker: None,
            streaming_dropped: None,
            streaming_write_failed: None,
            audio_prepared: false,
        };
        let analysis = prepare_for_dictation(&mut captured).expect("valid speech");
        let mean = captured.samples.iter().copied().sum::<f32>() / captured.samples.len() as f32;
        let peak = captured
            .samples
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);
        assert!(analysis.active_speech_ms >= 250);
        assert!(mean.abs() < 0.001);
        assert!(peak <= 0.709);
        assert!(peak > 0.60);
    }
}
