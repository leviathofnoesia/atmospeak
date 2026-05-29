use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use cpal::{
    SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::models::{MicrophoneInfo, RecordingStarted};

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
    _stream: Stream,
}

pub struct FinishedRecording {
    pub id: String,
    pub path: PathBuf,
    pub duration_ms: u64,
}

impl RecorderService {
    pub fn new(recordings_dir: PathBuf) -> Self {
        Self {
            recordings_dir,
            active: Mutex::new(None),
        }
    }

    pub fn list_microphones(&self) -> Result<Vec<MicrophoneInfo>> {
        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|device| device.name().ok());
        let mut microphones = Vec::new();

        for device in host.input_devices().context("failed to enumerate input devices")? {
            let name = device.name().unwrap_or_else(|_| "Unknown microphone".to_string());
            microphones.push(MicrophoneInfo {
                is_default: default_name.as_ref() == Some(&name),
                name,
            });
        }

        Ok(microphones)
    }

    pub fn start(&self, preferred_microphone: Option<String>) -> Result<RecordingStarted> {
        let mut active = self.active.lock();
        if active.is_some() {
            return Err(anyhow!("recording is already active"));
        }

        std::fs::create_dir_all(&self.recordings_dir)
            .context("failed to create recordings directory")?;

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
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let capture_samples = Arc::clone(&samples);
        let error_callback = |error| eprintln!("Wind Speak audio stream error: {error}");

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| capture_input(data, channels, &capture_samples),
                error_callback,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| capture_input(data, channels, &capture_samples),
                error_callback,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _| capture_input(data, channels, &capture_samples),
                error_callback,
                None,
            )?,
            format => return Err(anyhow!("unsupported microphone sample format: {format:?}")),
        };

        stream.play().context("failed to start microphone stream")?;

        let id = Uuid::new_v4().to_string();
        let started_at = Utc::now();
        *active = Some(ActiveRecording {
            id: id.clone(),
            started_instant: Instant::now(),
            sample_rate,
            samples,
            _stream: stream,
        });

        Ok(RecordingStarted {
            id,
            started_at,
            microphone_name,
        })
    }

    pub fn stop(&self) -> Result<FinishedRecording> {
        let active = self
            .active
            .lock()
            .take()
            .ok_or_else(|| anyhow!("no active recording to stop"))?;
        let duration_ms = active.started_instant.elapsed().as_millis() as u64;
        if Duration::from_millis(duration_ms) < Duration::from_millis(250) {
            return Err(anyhow!("recording was too short to transcribe"));
        }

        let samples = active.samples.lock().clone();
        if samples.is_empty() {
            return Err(anyhow!("microphone did not capture any samples"));
        }

        let resampled = resample_linear(&samples, active.sample_rate, TARGET_SAMPLE_RATE);
        let path = self.recordings_dir.join(format!("{}.wav", active.id));
        write_wav(&path, &resampled).context("failed to write recording wav")?;

        Ok(FinishedRecording {
            id: active.id,
            path,
            duration_ms,
        })
    }

    pub fn cancel(&self) -> Result<()> {
        let mut active = self.active.lock();
        if active.take().is_some() {
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
        let rms = (window
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>()
            / window.len() as f32)
            .sqrt();

        (rms * 4.0).clamp(0.0, 1.0)
    }
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

fn capture_input<T: Copy + IntoF32>(data: &[T], channels: usize, samples: &Arc<Mutex<Vec<f32>>>) {
    let mut output = samples.lock();
    for frame in data.chunks(channels.max(1)) {
        let sum = frame
            .iter()
            .map(|sample| (*sample).into_f32())
            .sum::<f32>();
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
