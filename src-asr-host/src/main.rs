use std::{
    io::{self, Read, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use atmospeak_asr_protocol::{
    AsrBackend, AsrCapabilities, AsrCommand, AsrEvent, MAX_FRAME_SIZE, PROTOCOL_VERSION,
    TranscriptionProfile,
};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    WhisperVadContext, WhisperVadContextParams, WhisperVadParams,
};

const SAMPLE_RATE: usize = 16_000;
const PREVIEW_INTERVAL: Duration = Duration::from_secs(1);
const PREVIEW_SAMPLES: usize = SAMPLE_RATE * 6;
const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE / 4;
const SILENCE_SAMPLES: usize = SAMPLE_RATE / 2;
const FORCE_SPLIT_SAMPLES: usize = SAMPLE_RATE * 15;
const OVERLAP_SAMPLES: usize = SAMPLE_RATE / 2;
const VAD_CHECK_SAMPLES: usize = SAMPLE_RATE / 10;
const VAD_MODEL_FILENAME: &str = "ggml-silero-v6.2.0.bin";

struct Model {
    context: WhisperContext,
    backend: AsrBackend,
    threads: i32,
    model_id: String,
    vad: WhisperVadContext,
}

struct Session {
    id: String,
    language: String,
    prompt: String,
    profile: TranscriptionProfile,
    audio: Vec<f32>,
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

fn main() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut model: Option<Model> = None;
    let mut session: Option<Session> = None;

    while let Some(command) = read_frame::<AsrCommand>(&mut input)? {
        match command {
            AsrCommand::Hello { protocol_version } => {
                if protocol_version != PROTOCOL_VERSION {
                    write_event(
                        &mut output,
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
                if backend == AsrBackend::Vulkan && !cfg!(feature = "vulkan") {
                    write_event(
                        &mut output,
                        &AsrEvent::Error {
                            session_id: None,
                            recoverable: true,
                            message: "this host was built without Vulkan".to_string(),
                        },
                    )?;
                    continue;
                }
                let mut parameters = WhisperContextParameters::default();
                parameters.use_gpu(backend == AsrBackend::Vulkan);
                parameters.flash_attn(true);
                let context = WhisperContext::new_with_params(&model_path, parameters)
                    .map_err(|error| anyhow!("failed to load model: {error}"))?;
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
                    .map_err(|error| {
                    anyhow!("failed to load VAD model {}: {error}", vad_path.display())
                })?;
                let model_id = std::path::Path::new(&model_path)
                    .file_stem()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "custom".to_string());
                model = Some(Model {
                    context,
                    backend,
                    threads: i32::from(threads.max(1)),
                    model_id,
                    vad,
                });
                write_event(
                    &mut output,
                    &AsrEvent::Ready {
                        capabilities: AsrCapabilities {
                            protocol_version: PROTOCOL_VERSION,
                            backend,
                            streaming: true,
                            vad: true,
                        },
                    },
                )?;
            }
            AsrCommand::StartSession {
                session_id,
                language,
                initial_prompt,
                profile,
            } => {
                if model.is_none() {
                    write_error(&mut output, Some(session_id), "model is not loaded", true)?;
                    continue;
                }
                session = Some(Session {
                    id: session_id,
                    language,
                    prompt: initial_prompt,
                    profile,
                    audio: Vec::new(),
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
                });
            }
            AsrCommand::AudioFrame {
                session_id,
                sequence,
                timestamp_ms,
                pcm_s16le,
            } => {
                let Some(active) = session.as_mut().filter(|active| active.id == session_id) else {
                    write_error(&mut output, Some(session_id), "session is not active", true)?;
                    continue;
                };
                if let Some(previous) = active.sequence
                    && sequence != previous + 1
                {
                    active.audio_frames_dropped += sequence.saturating_sub(previous + 1);
                    write_error(
                        &mut output,
                        Some(active.id.clone()),
                        &format!(
                            "audio sequence gap: expected {}, received {sequence}",
                            previous + 1
                        ),
                        true,
                    )?;
                }
                active.sequence = Some(sequence);
                let backlog_ms = active
                    .started
                    .elapsed()
                    .as_millis()
                    .saturating_sub(u128::from(timestamp_ms.saturating_add(20)))
                    as u64;
                active.max_backlog_ms = active.max_backlog_ms.max(backlog_ms);
                let frame = pcm_bytes_to_f32(&pcm_s16le)?;
                active.audio.extend_from_slice(&frame);

                let chunk_len = active.audio.len().saturating_sub(active.chunk_start);
                let mut finalize = chunk_len >= FORCE_SPLIT_SAMPLES;
                if chunk_len >= SAMPLE_RATE
                    && active.audio.len().saturating_sub(active.last_vad_check) >= VAD_CHECK_SAMPLES
                {
                    active.last_vad_check = active.audio.len();
                    let model = model.as_mut().expect("checked above");
                    let last_speech_end =
                        vad_last_speech_end(&mut model.vad, &active.audio[active.chunk_start..])?;
                    let speech_active = last_speech_end
                        .is_some_and(|end| chunk_len.saturating_sub(end) < SILENCE_SAMPLES);
                    if speech_active != active.speech_active {
                        active.speech_active = speech_active;
                        write_event(
                            &mut output,
                            &AsrEvent::SpeechState {
                                session_id: active.id.clone(),
                                active: speech_active,
                            },
                        )?;
                    }
                    finalize |= last_speech_end.is_some_and(|end| {
                        end >= MIN_SPEECH_SAMPLES
                            && chunk_len.saturating_sub(end) >= SILENCE_SAMPLES
                    });
                }
                if finalize {
                    let model = model.as_ref().expect("checked above");
                    finalize_chunk(model, active, &mut output)?;
                } else if active.audio.len() >= SAMPLE_RATE
                    && backlog_ms < 5_000
                    && active.last_preview.elapsed()
                        >= if backlog_ms > 2_000 {
                            PREVIEW_INTERVAL * 2
                        } else {
                            PREVIEW_INTERVAL
                        }
                {
                    let model = model.as_ref().expect("checked above");
                    emit_preview(model, active, &mut output)?;
                }
            }
            AsrCommand::StopSession { session_id } => {
                let Some(mut active) = session.take().filter(|active| active.id == session_id)
                else {
                    write_error(&mut output, Some(session_id), "session is not active", true)?;
                    continue;
                };
                let finalize_started = Instant::now();
                let tail_audio_ms =
                    samples_to_ms(active.audio.len().saturating_sub(active.chunk_start));
                let tail_has_speech = if active.audio.len() > active.chunk_start {
                    let model = model
                        .as_mut()
                        .ok_or_else(|| anyhow!("model is not loaded"))?;
                    vad_last_speech_end(&mut model.vad, &active.audio[active.chunk_start..])?
                        .is_some()
                } else {
                    false
                };
                if tail_has_speech && active.audio.len() > active.chunk_start + SAMPLE_RATE / 10 {
                    finalize_chunk(
                        model
                            .as_ref()
                            .ok_or_else(|| anyhow!("model is not loaded"))?,
                        &mut active,
                        &mut output,
                    )?;
                }
                let audio_ms = samples_to_ms(active.audio.len());
                let processed_during_recording_ms = audio_ms.saturating_sub(tail_audio_ms);
                let model = model
                    .as_ref()
                    .ok_or_else(|| anyhow!("model is not loaded"))?;
                write_event(
                    &mut output,
                    &AsrEvent::Metrics(atmospeak_asr_protocol::StreamingMetrics {
                        session_id: active.id.clone(),
                        backend: model.backend,
                        model_id: model.model_id.clone(),
                        first_partial_ms: active.first_partial_ms,
                        stop_ack_ms: 0,
                        finalize_ms: finalize_started.elapsed().as_millis() as u64,
                        paste_ms: 0,
                        processed_during_recording_ms,
                        tail_audio_ms,
                        max_backlog_ms: active.max_backlog_ms,
                        audio_frames_dropped: active.audio_frames_dropped,
                        fallback_reason: None,
                    }),
                )?;
                write_event(
                    &mut output,
                    &AsrEvent::Final {
                        session_id: active.id,
                        text: active.committed.trim().to_string(),
                        processed_during_recording_ms,
                        tail_audio_ms,
                    },
                )?;
                eprintln!(
                    "finalized streaming session in {}ms",
                    finalize_started.elapsed().as_millis()
                );
            }
            AsrCommand::CancelSession { session_id } => {
                if session
                    .as_ref()
                    .is_some_and(|active| active.id == session_id)
                {
                    session = None;
                }
            }
            AsrCommand::Shutdown => break,
        }
    }
    Ok(())
}

fn emit_preview<W: Write>(model: &Model, session: &mut Session, output: &mut W) -> Result<()> {
    let start = session.audio.len().saturating_sub(PREVIEW_SAMPLES);
    let text = transcribe(
        model,
        &session.audio[start..],
        &session.language,
        &prompt(session),
        session.profile,
    )?;
    let text = strip_overlap(&session.committed, &text);
    session.revision += 1;
    session.last_preview = Instant::now();
    let latency = session.started.elapsed().as_millis() as u64;
    session.first_partial_ms.get_or_insert(latency);
    write_event(
        output,
        &AsrEvent::Partial {
            session_id: session.id.clone(),
            revision: session.revision,
            text,
            covered_through_ms: samples_to_ms(session.audio.len()),
        },
    )
}

fn finalize_chunk<W: Write>(model: &Model, session: &mut Session, output: &mut W) -> Result<()> {
    let end = session.audio.len();
    let text = transcribe(
        model,
        &session.audio[session.chunk_start..end],
        &session.language,
        &prompt(session),
        session.profile,
    )?;
    session.committed = merge_overlap(&session.committed, &text);
    let start_ms = samples_to_ms(session.chunk_start);
    let end_ms = samples_to_ms(end);
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
    session.segment_index += 1;
    session.chunk_start = end.saturating_sub(OVERLAP_SAMPLES);
    session.last_vad_check = session.chunk_start;
    session.speech_active = false;
    Ok(())
}

fn vad_last_speech_end(vad: &mut WhisperVadContext, audio: &[f32]) -> Result<Option<usize>> {
    if audio.len() < MIN_SPEECH_SAMPLES {
        return Ok(None);
    }
    let padded;
    let vad_audio = if audio.len() < SAMPLE_RATE {
        padded = audio
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, SAMPLE_RATE - audio.len()))
            .collect::<Vec<_>>();
        padded.as_slice()
    } else {
        audio
    };
    let mut params = WhisperVadParams::new();
    params.set_threshold(0.50);
    params.set_min_speech_duration(250);
    params.set_min_silence_duration(500);
    params.set_max_speech_duration(15.0);
    params.set_speech_pad(200);
    params.set_samples_overlap(0.5);
    let segments = vad
        .segments_from_samples(params, vad_audio)
        .map_err(|error| anyhow!("VAD inference failed: {error}"))?;
    Ok(segments
        .map(|segment| ((segment.end * SAMPLE_RATE as f32 / 100.0) as usize).min(audio.len()))
        .last())
}

fn transcribe(
    model: &Model,
    audio: &[f32],
    language: &str,
    prompt: &str,
    profile: TranscriptionProfile,
) -> Result<String> {
    if audio.len() < SAMPLE_RATE / 10 {
        return Ok(String::new());
    }
    let mut state: WhisperState = model
        .context
        .create_state()
        .map_err(|error| anyhow!("failed to create decoder state: {error}"))?;
    let best_of = match profile {
        TranscriptionProfile::Speed => 1,
        TranscriptionProfile::Balanced if model.backend == AsrBackend::Vulkan => 2,
        TranscriptionProfile::Balanced => 1,
        TranscriptionProfile::Quality => 3,
    };
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of });
    params.set_n_threads(model.threads);
    params.set_language(Some(if language.is_empty() { "en" } else { language }));
    params.set_initial_prompt(prompt);
    params.set_no_context(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    state
        .full(params, audio)
        .map_err(|error| anyhow!("Whisper inference failed: {error}"))?;
    Ok(state
        .as_iter()
        .map(|segment| segment.to_string())
        .collect::<String>()
        .trim()
        .to_string())
}

fn prompt(session: &Session) -> String {
    let combined = format!("{} {}", session.prompt, session.committed);
    let words = combined.split_whitespace().collect::<Vec<_>>();
    let start = words.len().saturating_sub(160);
    words[start..].join(" ")
}

fn merge_overlap(committed: &str, next: &str) -> String {
    let left = committed.split_whitespace().collect::<Vec<_>>();
    let right = next.split_whitespace().collect::<Vec<_>>();
    let overlap = overlap_count(&left, &right);
    left.into_iter()
        .chain(right.into_iter().skip(overlap))
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_overlap(committed: &str, next: &str) -> String {
    let left = committed.split_whitespace().collect::<Vec<_>>();
    let right = next.split_whitespace().collect::<Vec<_>>();
    let overlap = overlap_count(&left, &right);
    right
        .into_iter()
        .skip(overlap)
        .collect::<Vec<_>>()
        .join(" ")
}

fn overlap_count(left: &[&str], right: &[&str]) -> usize {
    let maximum = left.len().min(right.len()).min(12);
    (1..=maximum)
        .rev()
        .find(|count| {
            left[left.len() - count..]
                .iter()
                .zip(&right[..*count])
                .all(|(a, b)| normalize(a) == normalize(b))
        })
        .unwrap_or(0)
}

fn normalize(token: &str) -> String {
    token
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn pcm_bytes_to_f32(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() % 2 != 0 {
        bail!("PCM frame has an odd byte count");
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / i16::MAX as f32)
        .collect())
}

fn samples_to_ms(samples: usize) -> u64 {
    ((samples as u128 * 1_000) / SAMPLE_RATE as u128) as u64
}

fn read_frame<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> Result<Option<T>> {
    let mut length = [0_u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_SIZE {
        bail!("invalid IPC frame length: {length}");
    }
    let mut payload = vec![0; length];
    input.read_exact(&mut payload)?;
    rmp_serde::from_slice(&payload)
        .context("invalid IPC frame")
        .map(Some)
}

fn write_event(output: &mut impl Write, event: &AsrEvent) -> Result<()> {
    let payload = rmp_serde::to_vec_named(event)?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("outgoing IPC frame exceeds limit");
    }
    output.write_all(&(payload.len() as u32).to_le_bytes())?;
    output.write_all(&payload)?;
    output.flush()?;
    Ok(())
}

fn write_error(
    output: &mut impl Write,
    session_id: Option<String>,
    message: &str,
    recoverable: bool,
) -> Result<()> {
    write_event(
        output,
        &AsrEvent::Error {
            session_id,
            recoverable,
            message: message.to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_merge_removes_only_matching_boundary_tokens() {
        assert_eq!(
            merge_overlap("the porcelain moon", "porcelain moon hums"),
            "the porcelain moon hums"
        );
        assert_eq!(
            strip_overlap("the porcelain moon", "porcelain moon hums"),
            "hums"
        );
        assert_eq!(merge_overlap("very very", "very good"), "very very good");
    }

    #[test]
    fn pcm_frames_require_complete_samples() {
        assert!(pcm_bytes_to_f32(&[1]).is_err());
        assert_eq!(pcm_bytes_to_f32(&[0, 0, 255, 127]).unwrap().len(), 2);
    }
}
