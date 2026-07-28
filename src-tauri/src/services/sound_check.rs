use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{AudioCalibrationRecord, SoundCheckResult},
    services::{app_state::AppState, metrics, recorder},
};

pub const EXPECTED_PHRASE: &str = "The porcelain moon hums over the studio.";

pub fn start(app: &AppHandle, device_name: String) -> Result<()> {
    let device_name = device_name.trim();
    if device_name.is_empty() {
        return Err(anyhow!(
            "Choose a microphone before starting the sound check."
        ));
    }
    let state = app.state::<AppState>();
    state
        .recorder
        .start(Some(device_name.to_string()), None)
        .map_err(classify_capture_error)?;
    let persist_result = (|| -> Result<()> {
        let mut settings = state.database.lock().load_settings()?;
        settings.microphone_name = Some(device_name.to_string());
        settings.audio_calibration = None;
        state.database.lock().save_settings(&settings)?;
        Ok(())
    })();
    if let Err(error) = persist_result {
        let _ = state.recorder.cancel();
        return Err(error);
    }
    start_level_events(app);
    metrics::emit_runtime(
        app,
        "sound-check-started",
        format!("Calibrating microphone: {device_name}"),
    );
    Ok(())
}

pub fn cancel(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.end_level_stream();
    let cancelled = state.recorder.cancel().is_ok();
    if cancelled {
        metrics::emit_runtime(app, "sound-check-cancelled", "Sound check cancelled.");
    }
    cancelled
}

pub fn finish(app: &AppHandle, expected_phrase: String) -> Result<SoundCheckResult> {
    let total_started = Instant::now();
    let state = app.state::<AppState>();
    state.end_level_stream();

    let capture_started = Instant::now();
    let mut captured = state.recorder.stop().map_err(classify_capture_error)?;
    recorder::finalize_capture(&mut captured).map_err(classify_capture_error)?;
    let capture_ms = capture_started.elapsed().as_millis() as u64;
    let device_name = state
        .database
        .lock()
        .load_settings()
        .ok()
        .and_then(|settings| settings.microphone_name)
        .unwrap_or_else(|| "Selected microphone".to_string());
    let capture_format = format!("mono f32 {}Hz -> mono PCM16 16000Hz", captured.sample_rate);
    let analysis = recorder::analyze_samples(&captured.samples, captured.sample_rate);
    let duration_ms = captured.duration_ms;
    let model_id = state
        .database
        .lock()
        .load_settings()
        .map(|settings| settings.active_model_id)
        .unwrap_or_else(|_| "base.en".to_string());

    let initial_failure = quality_failure(duration_ms, &analysis);
    if let Some(code) = initial_failure {
        return Ok(emit_result(
            app,
            SoundCheckResult {
                passed: false,
                failure_code: Some(code.to_string()),
                device_name,
                capture_format,
                duration_ms,
                active_speech_ms: analysis.active_speech_ms,
                rms_dbfs: analysis.rms_dbfs,
                peak_dbfs: analysis.peak_dbfs,
                noise_floor_dbfs: analysis.noise_floor_dbfs,
                snr_db: analysis.snr_db,
                clipping_ratio: analysis.clipping_ratio,
                transcript: String::new(),
                expected_phrase,
                token_similarity: 0.0,
                asr_backend: String::new(),
                model_id,
                capture_ms,
                asr_ms: 0,
                total_ms: total_started.elapsed().as_millis() as u64,
            },
        ));
    }

    let path = captured.path.clone();
    let finished = recorder::finish_recording(captured)?;
    let expected = if expected_phrase.trim().is_empty() {
        EXPECTED_PHRASE.to_string()
    } else {
        expected_phrase
    };
    let Some(host) = state.asr_host() else {
        let _ = std::fs::remove_file(&path);
        return Ok(emit_result(
            app,
            SoundCheckResult {
                passed: false,
                failure_code: Some("backend_unavailable".to_string()),
                device_name,
                capture_format,
                duration_ms,
                active_speech_ms: analysis.active_speech_ms,
                rms_dbfs: analysis.rms_dbfs,
                peak_dbfs: analysis.peak_dbfs,
                noise_floor_dbfs: analysis.noise_floor_dbfs,
                snr_db: analysis.snr_db,
                clipping_ratio: analysis.clipping_ratio,
                transcript: String::new(),
                expected_phrase: expected,
                token_similarity: 0.0,
                asr_backend: String::new(),
                model_id,
                capture_ms,
                asr_ms: 0,
                total_ms: total_started.elapsed().as_millis() as u64,
            },
        ));
    };

    let asr_started = Instant::now();
    let transcript = match host.transcribe(&finished.path) {
        Ok(transcript) => transcript,
        Err(error) => {
            let asr_ms = asr_started.elapsed().as_millis() as u64;
            let _ = std::fs::remove_file(&finished.path);
            metrics::emit_runtime(
                app,
                "sound-check-host-error",
                format!("Resident transcription host failed: {error}"),
            );
            return Ok(emit_result(
                app,
                SoundCheckResult {
                    passed: false,
                    failure_code: Some("backend_unavailable".to_string()),
                    device_name,
                    capture_format,
                    duration_ms,
                    active_speech_ms: analysis.active_speech_ms,
                    rms_dbfs: analysis.rms_dbfs,
                    peak_dbfs: analysis.peak_dbfs,
                    noise_floor_dbfs: analysis.noise_floor_dbfs,
                    snr_db: analysis.snr_db,
                    clipping_ratio: analysis.clipping_ratio,
                    transcript: String::new(),
                    expected_phrase: expected,
                    token_similarity: 0.0,
                    asr_backend: metrics::ASR_BACKEND_HOST.to_string(),
                    model_id,
                    capture_ms,
                    asr_ms,
                    total_ms: total_started.elapsed().as_millis() as u64,
                },
            ));
        }
    };
    let asr_ms = asr_started.elapsed().as_millis() as u64;
    let _ = std::fs::remove_file(&finished.path);

    let similarity = token_similarity(&expected, &transcript);
    let lexical_count = lexical_tokens(&transcript).len();
    let sound_only = transcript.trim().is_empty()
        || (transcript.trim().starts_with('[') && transcript.trim().ends_with(']'));
    let passed = lexical_count >= 4 && similarity >= 0.50 && !sound_only;
    let result = SoundCheckResult {
        passed,
        failure_code: (!passed).then(|| "unintelligible_phrase".to_string()),
        device_name: device_name.clone(),
        capture_format,
        duration_ms,
        active_speech_ms: analysis.active_speech_ms,
        rms_dbfs: analysis.rms_dbfs,
        peak_dbfs: analysis.peak_dbfs,
        noise_floor_dbfs: analysis.noise_floor_dbfs,
        snr_db: analysis.snr_db,
        clipping_ratio: analysis.clipping_ratio,
        transcript,
        expected_phrase: expected,
        token_similarity: similarity,
        asr_backend: metrics::ASR_BACKEND_HOST.to_string(),
        model_id: model_id.clone(),
        capture_ms,
        asr_ms,
        total_ms: total_started.elapsed().as_millis() as u64,
    };

    if result.passed {
        let mut settings = state.database.lock().load_settings()?;
        settings.microphone_name = Some(device_name.clone());
        settings.audio_calibration = Some(AudioCalibrationRecord {
            device_name,
            checked_at: Utc::now(),
            rms_dbfs: result.rms_dbfs,
            peak_dbfs: result.peak_dbfs,
            snr_db: result.snr_db,
            model_id,
            asr_backend: result.asr_backend.clone(),
        });
        state.database.lock().save_settings(&settings)?;
    }

    Ok(emit_result(app, result))
}

pub fn start_level_events(app: &AppHandle) {
    let generation = app.state::<AppState>().begin_level_stream();
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("atmospeak-mic-level".to_string())
        .spawn(move || {
            loop {
                let Some(state) = app.try_state::<AppState>() else {
                    break;
                };
                if !state.level_stream_is_current(generation) {
                    break;
                }
                let level = state.recorder.mic_level();
                let _ = app.emit("atmospeak://mic-level", level.clone());
                let _ = app.emit("wind-speak://mic-level", level);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
}

fn emit_result(app: &AppHandle, result: SoundCheckResult) -> SoundCheckResult {
    let _ = app.emit("atmospeak://sound-check", result.clone());
    let _ = app.emit("wind-speak://sound-check", result.clone());
    metrics::emit_runtime(
        app,
        if result.passed {
            "sound-check-passed"
        } else {
            "sound-check-failed"
        },
        format!(
            "device={} rms={:.1}dBFS peak={:.1}dBFS snr={:.1}dB backend={} result={}",
            result.device_name,
            result.rms_dbfs,
            result.peak_dbfs,
            result.snr_db,
            result.asr_backend,
            result.failure_code.as_deref().unwrap_or("passed")
        ),
    );
    result
}

fn quality_failure(duration_ms: u64, analysis: &recorder::AudioAnalysis) -> Option<&'static str> {
    if duration_ms < 2_000 {
        Some("too_short")
    } else if duration_ms > 12_000 {
        Some("too_long")
    } else if analysis.active_speech_ms < 1_200
        || analysis.rms_dbfs < -42.0
        || analysis.peak_dbfs < -30.0
    {
        Some("too_quiet")
    } else if analysis.snr_db < 12.0 {
        Some("excessive_noise")
    } else if analysis.clipping_ratio > 0.001 || analysis.peak_dbfs >= -1.0 {
        Some("clipping")
    } else {
        None
    }
}

fn classify_capture_error(error: anyhow::Error) -> anyhow::Error {
    let message = error.to_string();
    if message.contains("configured microphone not found") {
        anyhow!("The selected microphone is unavailable or disconnected.")
    } else if message.contains("no default microphone") {
        anyhow!("No microphone is available.")
    } else if message.contains("permission") || message.contains("access") {
        anyhow!("Microphone permission was denied by Windows.")
    } else {
        error
    }
}

fn lexical_tokens(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

fn token_similarity(expected: &str, actual: &str) -> f32 {
    let expected = lexical_tokens(expected);
    let actual = lexical_tokens(actual);
    let denominator = expected.len().max(actual.len());
    if denominator == 0 {
        return 0.0;
    }
    1.0 - token_distance(&expected, &actual) as f32 / denominator as f32
}

fn token_distance(left: &[String], right: &[String]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_token) in left.iter().enumerate() {
        let mut current = vec![left_index + 1; right.len() + 1];
        for (right_index, right_token) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_token != right_token));
        }
        previous = current;
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::{EXPECTED_PHRASE, quality_failure, token_similarity};
    use crate::services::recorder::AudioAnalysis;

    #[test]
    fn phrase_similarity_tolerates_one_missed_word() {
        assert!(token_similarity(EXPECTED_PHRASE, "the porcelain moon hums over studio") > 0.7);
    }

    #[test]
    fn sound_effect_placeholder_does_not_match() {
        assert_eq!(token_similarity(EXPECTED_PHRASE, "[SOUND]"), 0.0);
    }

    #[test]
    fn sound_check_thresholds_report_the_actionable_failure() {
        let healthy = AudioAnalysis {
            active_speech_ms: 2_000,
            rms_dbfs: -24.0,
            peak_dbfs: -8.0,
            noise_floor_dbfs: -52.0,
            snr_db: 28.0,
            clipping_ratio: 0.0,
        };
        assert_eq!(quality_failure(4_000, &healthy), None);
        assert_eq!(quality_failure(1_999, &healthy), Some("too_short"));
        assert_eq!(quality_failure(12_001, &healthy), Some("too_long"));
        assert_eq!(
            quality_failure(
                4_000,
                &AudioAnalysis {
                    rms_dbfs: -45.0,
                    peak_dbfs: -33.0,
                    ..healthy.clone()
                }
            ),
            Some("too_quiet")
        );
        assert_eq!(
            quality_failure(
                4_000,
                &AudioAnalysis {
                    snr_db: 6.0,
                    ..healthy.clone()
                }
            ),
            Some("excessive_noise")
        );
        assert_eq!(
            quality_failure(
                4_000,
                &AudioAnalysis {
                    peak_dbfs: -0.4,
                    clipping_ratio: 0.02,
                    ..healthy
                }
            ),
            Some("clipping")
        );
    }
}
