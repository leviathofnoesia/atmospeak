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

use crate::models::{MicLevel, MicrophoneInfo, RecordingStarted};

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

pub struct CapturedRecording {
    pub id: String,
    pub path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
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
        let error_callback = |error| eprintln!("Atmospeak audio stream error: {error}");

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

    pub fn stop(&self) -> Result<CapturedRecording> {
        let active = self
            .active
            .lock()
            .take()
            .ok_or_else(|| anyhow!("no active recording to stop"))?;
        let duration_ms = active.started_instant.elapsed().as_millis() as u64;
        if Duration::from_millis(duration_ms) < Duration::from_millis(250) {
            return Err(anyhow!("recording was too short to transcribe"));
        }

        let ActiveRecording {
            id,
            sample_rate,
            samples,
            _stream,
            ..
        } = active;
        drop(_stream);

        let samples = match Arc::try_unwrap(samples) {
            Ok(samples) => samples.into_inner(),
            Err(samples) => samples.lock().clone(),
        };
        if samples.is_empty() {
            return Err(anyhow!("microphone did not capture any samples"));
        }

        Ok(CapturedRecording {
            path: self.recordings_dir.join(format!("{id}.wav")),
            id,
            duration_ms,
            sample_rate,
            samples,
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
    let resampled = resample_linear(&captured.samples, captured.sample_rate, TARGET_SAMPLE_RATE);
    write_wav(&captured.path, &resampled).context("failed to write recording wav")?;

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
        AudioAnalysis, CapturedRecording, TARGET_SAMPLE_RATE, analyze_samples, finish_recording,
        normal_dictation_failure, prepare_for_dictation,
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
