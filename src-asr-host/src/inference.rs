//! Decode and text-merging primitives shared by the session worker. Everything
//! here is either pure or takes its whisper state explicitly, so it can be
//! unit-tested without a running session.

use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{Result, anyhow, bail};
use atmospeak_asr_protocol::{AsrBackend, AsrEvent, MAX_FRAME_SIZE, TranscriptionProfile};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperState, WhisperVadContext, WhisperVadParams,
};

pub const SAMPLE_RATE: usize = 16_000;

unsafe extern "C" fn abort_callback_trampoline(user_data: *mut std::ffi::c_void) -> bool {
    if user_data.is_null() {
        return false;
    }
    // Safety: caller passes a live AtomicBool pointer for the duration of whisper_full.
    unsafe { (*(user_data as *const AtomicBool)).load(Ordering::Acquire) }
}

/// Bound Silero's input to the trailing seconds of an uncommitted chunk. A
/// pause that matters for finalization (≥500 ms silence after ≥250 ms speech)
/// always ends inside the last 3 s; anything older was already evaluated by
/// the 100 ms cadence, so scanning the full chunk again is wasted work.
pub const VAD_WINDOW_SAMPLES: usize = SAMPLE_RATE * 3;

/// First sample of the slice worth VAD-scanning within the current chunk.
pub fn vad_window_start(chunk_start: usize, audio_len: usize) -> usize {
    audio_len.saturating_sub(VAD_WINDOW_SAMPLES).max(chunk_start)
}

pub struct DecodedText {
    pub text: String,
    pub confirmed_overlap_words: usize,
}

pub fn vad_last_speech_end(vad: &mut WhisperVadContext, audio: &[f32]) -> Result<Option<usize>> {
    const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE / 4;
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

#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    state: &mut WhisperState,
    backend: AsrBackend,
    threads: i32,
    audio: &[f32],
    language: &str,
    prompt: &str,
    profile: TranscriptionProfile,
    leading_overlap_samples: usize,
    abort: Option<&Arc<AtomicBool>>,
) -> Result<DecodedText> {
    if audio.len() < SAMPLE_RATE / 10 {
        return Ok(DecodedText {
            text: String::new(),
            confirmed_overlap_words: 0,
        });
    }
    if abort.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        bail!("whisper decode aborted");
    }
    let best_of = match profile {
        TranscriptionProfile::Speed => 1,
        TranscriptionProfile::Balanced if backend == AsrBackend::Vulkan => 2,
        TranscriptionProfile::Balanced => 1,
        TranscriptionProfile::Quality => 3,
    };
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of });
    params.set_n_threads(threads);
    params.set_language(Some(if language.is_empty() { "en" } else { language }));
    params.set_initial_prompt(prompt);
    params.set_no_context(true);
    // Streaming chunks are short; single-segment + no timestamps cuts encoder
    // work on pure-fresh audio. With retained acoustic overlap we need real
    // segment timestamps so confirmed_overlap_words can drop the re-emitted
    // prefix — single_segment collapses overlap+fresh into one segment and
    // leaves confirmed_overlap at 0 (duplicated words at chunk boundaries).
    if leading_overlap_samples == 0 {
        params.set_single_segment(true);
        params.set_no_timestamps(true);
    } else {
        params.set_single_segment(false);
        params.set_no_timestamps(false);
    }
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    // ggml_abort_callback: return true to abort. StopSession sets this flag so
    // an in-flight force-split cannot block release→paste for seconds.
    if let Some(flag) = abort {
        // Safety: `flag` outlives `state.full` below; whisper only calls this
        // during that decode. Prefer a raw AtomicBool pointer over whisper-rs's
        // boxed trampoline, which has a known type mismatch with dyn FnMut.
        unsafe {
            params.set_abort_callback(Some(abort_callback_trampoline));
            params.set_abort_callback_user_data(
                Arc::as_ptr(flag) as *const AtomicBool as *mut std::ffi::c_void,
            );
        }
    }
    state
        .full(params, audio)
        .map_err(|error| anyhow!("Whisper inference failed: {error}"))?;
    if abort.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        bail!("whisper decode aborted");
    }
    let overlap_end_timestamp =
        i64::try_from(samples_to_ms(leading_overlap_samples) / 10).unwrap_or(i64::MAX);
    let mut text = String::new();
    let mut confirmed_overlap = String::new();
    for segment in state.as_iter() {
        let segment_text = segment.to_string();
        text.push_str(&segment_text);
        // Only text from segments ending entirely inside the retained acoustic
        // overlap is eligible for removal. A segment crossing into fresh audio
        // is kept intact so intentional repeated words cannot be discarded.
        if leading_overlap_samples > 0
            && segment.end_timestamp() >= 0
            && segment.end_timestamp() <= overlap_end_timestamp
        {
            confirmed_overlap.push_str(&segment_text);
        }
    }
    Ok(DecodedText {
        text: text.trim().to_string(),
        confirmed_overlap_words: confirmed_overlap.split_whitespace().count(),
    })
}

pub fn merge_overlap(committed: &str, next: &str, confirmed_overlap_words: usize) -> String {
    let mut left = committed.split_whitespace().collect::<Vec<_>>();
    let right = next.split_whitespace().collect::<Vec<_>>();
    let overlap = overlap_count(&left, &right, confirmed_overlap_words);
    left.truncate(left.len().saturating_sub(overlap));
    left.into_iter().chain(right).collect::<Vec<_>>().join(" ")
}

fn overlap_count(left: &[&str], right: &[&str], confirmed_overlap_words: usize) -> usize {
    // When acoustic overlap confirmed at least one boundary word, allow the
    // longest matching suffix/prefix (Whisper often re-emits past the overlap
    // zone). With zero confirmed words, do not soft-match — intentional
    // repeats across chunk boundaries must be preserved.
    let maximum = if confirmed_overlap_words >= 1 {
        left.len().min(right.len()).min(12)
    } else {
        0
    };
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

pub fn pcm_bytes_to_f32(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() % 2 != 0 {
        bail!("PCM frame has an odd byte count");
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / i16::MAX as f32)
        .collect())
}

pub fn samples_to_ms(samples: usize) -> u64 {
    ((samples as u128 * 1_000) / SAMPLE_RATE as u128) as u64
}

pub fn write_event(output: &mut impl Write, event: &AsrEvent) -> Result<()> {
    let payload = rmp_serde::to_vec_named(event)?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("outgoing IPC frame exceeds limit");
    }
    output.write_all(&(payload.len() as u32).to_le_bytes())?;
    output.write_all(&payload)?;
    output.flush()?;
    Ok(())
}

pub fn write_error(
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
            merge_overlap("the porcelain moon", "porcelain moon hums", 2),
            "the porcelain moon hums"
        );
        // Zero confirmed overlap: keep intentional boundary doubles.
        assert_eq!(
            merge_overlap("very very", "very good", 0),
            "very very very good"
        );
        // Confirmed >= 1: soft-extend to the longest matching boundary.
        assert_eq!(
            merge_overlap("very very", "very good", 1),
            "very very good"
        );
        assert_eq!(
            merge_overlap("very very", "very very good", 1),
            "very very good"
        );
        assert_eq!(
            merge_overlap("wait, for me", "wait for me now", 3),
            "wait for me now"
        );
    }

    #[test]
    fn pcm_frames_require_complete_samples() {
        assert!(pcm_bytes_to_f32(&[1]).is_err());
        assert_eq!(pcm_bytes_to_f32(&[0, 0, 255, 127]).unwrap().len(), 2);
    }

    #[test]
    fn vad_window_covers_short_chunks_entirely() {
        // Chunk shorter than the window: scan from the chunk start.
        assert_eq!(vad_window_start(0, SAMPLE_RATE), 0);
        assert_eq!(vad_window_start(500, 2 * SAMPLE_RATE), 500);
    }

    #[test]
    fn vad_window_bounds_long_chunks_to_the_tail() {
        let chunk_start = 10_000;
        let audio_len = chunk_start + VAD_WINDOW_SAMPLES * 3;
        // Chunk longer than the window: only the trailing 3 s are scanned.
        assert_eq!(vad_window_start(chunk_start, audio_len), audio_len - VAD_WINDOW_SAMPLES);
        // The window never starts before the chunk itself.
        assert_eq!(
            vad_window_start(audio_len - SAMPLE_RATE, audio_len),
            audio_len - SAMPLE_RATE
        );
    }
}
