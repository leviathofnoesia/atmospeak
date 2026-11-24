use parking_lot::Mutex;

use crate::models::LiveTranscriptEvent;
use crate::services::text_processing::DictationTextContext;

/// Paste-ready hypothesis built during listening from stable + partial ASR events.
#[derive(Debug, Clone)]
pub struct LivePasteSnapshot {
    pub session_id: String,
    pub raw_text: String,
    pub paste_text: String,
    pub covered_through_ms: u64,
}

#[derive(Debug, Default)]
struct LivePasteState {
    context: Option<DictationTextContext>,
    session_id: String,
    stable_text: String,
    partial_text: String,
    paste_text: String,
    raw_text: String,
    covered_through_ms: u64,
    revision: u64,
}

/// Shared buffer updated on every live ASR event; read on release for aggressive paste.
#[derive(Debug, Default)]
pub struct LivePasteBuffer {
    inner: Mutex<LivePasteState>,
}

impl LivePasteBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_session(&self, session_id: impl Into<String>, context: DictationTextContext) {
        let mut state = self.inner.lock();
        *state = LivePasteState {
            context: Some(context),
            session_id: session_id.into(),
            ..LivePasteState::default()
        };
    }

    pub fn clear(&self) {
        *self.inner.lock() = LivePasteState::default();
    }

    /// Snapshot of the current display text for the durable History checkpoint.
    pub fn take_paste_ready(&self) -> Option<LivePasteSnapshot> {
        let state = self.inner.lock();
        let paste_text = state.paste_text.trim();
        if paste_text.is_empty() {
            return None;
        }
        Some(LivePasteSnapshot {
            session_id: state.session_id.clone(),
            raw_text: state.raw_text.clone(),
            paste_text: paste_text.to_string(),
            covered_through_ms: state.covered_through_ms,
        })
    }

    pub fn apply_partial(
        &self,
        session_id: &str,
        text: &str,
        covered_through_ms: u64,
        first_partial_latency_ms: Option<u64>,
        revision_hint: u64,
    ) -> Option<LiveTranscriptEvent> {
        let mut state = self.inner.lock();
        if state.session_id != session_id {
            return None;
        }
        // ASR events can arrive out of order when the sidecar is flushing its
        // bounded queue. A late hypothesis must never replace newer text or
        // move the coverage cursor backwards.
        if (revision_hint > 0 && revision_hint < state.revision)
            || covered_through_ms < state.covered_through_ms
        {
            return None;
        }
        let unchanged = state.partial_text == text;
        state.revision = state.revision.saturating_add(1).max(revision_hint);
        if unchanged {
            state.covered_through_ms = covered_through_ms;
            return Some(transcript_event(&state, first_partial_latency_ms));
        }
        text.clone_into(&mut state.partial_text);
        Some(recompute_and_event(
            &mut state,
            covered_through_ms,
            first_partial_latency_ms,
        ))
    }

    pub fn apply_stable(
        &self,
        session_id: &str,
        text: &str,
        covered_through_ms: u64,
        first_partial_latency_ms: Option<u64>,
    ) -> Option<LiveTranscriptEvent> {
        let mut state = self.inner.lock();
        if state.session_id != session_id {
            return None;
        }
        if covered_through_ms < state.covered_through_ms {
            return None;
        }
        state.stable_text = text.to_string();
        state.partial_text.clear();
        state.revision = state.revision.saturating_add(1);
        Some(recompute_and_event(
            &mut state,
            covered_through_ms,
            first_partial_latency_ms,
        ))
    }
}

fn join_raw(stable: &str, partial: &str) -> String {
    let mut raw = String::with_capacity(stable.len() + partial.len());
    for word in stable.split_whitespace().chain(partial.split_whitespace()) {
        if !raw.is_empty() {
            raw.push(' ');
        }
        raw.push_str(word);
    }
    raw
}

fn recompute_and_event(
    state: &mut LivePasteState,
    covered_through_ms: u64,
    first_partial_latency_ms: Option<u64>,
) -> LiveTranscriptEvent {
    let raw = join_raw(&state.stable_text, &state.partial_text);
    state.covered_through_ms = covered_through_ms;
    let paste = match state.context.as_ref() {
        Some(context) => context.prepare_text(&raw),
        None => raw.trim().to_string(),
    };
    state.raw_text = raw;
    state.paste_text = paste;
    transcript_event(state, first_partial_latency_ms)
}

fn transcript_event(
    state: &LivePasteState,
    first_partial_latency_ms: Option<u64>,
) -> LiveTranscriptEvent {
    // Emit the full paste-ready string as stable so the orb matches what release pastes.
    LiveTranscriptEvent {
        session_id: state.session_id.clone(),
        revision: state.revision,
        stable_text: state.paste_text.clone(),
        partial_text: String::new(),
        covered_through_ms: state.covered_through_ms,
        first_partial_latency_ms,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use crate::models::{DictionaryEntry, Snippet};

    use super::*;

    fn sample_context() -> DictationTextContext {
        DictationTextContext {
            cleanup_enabled: true,
            dictionary: vec![DictionaryEntry {
                id: "1".to_string(),
                phrase: "bridge mind".to_string(),
                replacement: "BridgeMind".to_string(),
                enabled: true,
                created_at: Utc::now(),
            }],
            snippets: vec![Snippet {
                id: "1".to_string(),
                trigger: "ship intro".to_string(),
                body: "Thanks for the review.".to_string(),
                enabled: true,
                created_at: Utc::now(),
            }],
        }
    }

    #[test]
    fn cleans_stable_plus_partial_for_paste() {
        let buffer = LivePasteBuffer::new();
        let context = sample_context();
        buffer.begin_session("s1", context.clone());

        let stable = buffer
            .apply_stable("s1", "um bridge mind", 1000, None)
            .expect("stable event");
        assert!(stable.stable_text.contains("BridgeMind"));
        assert!(stable.partial_text.is_empty());

        let partial = buffer
            .apply_partial("s1", "comma ship intro", 1500, Some(40), 2)
            .expect("partial event");
        assert_eq!(partial.stable_text, "BridgeMind, Thanks for the review.");

        let ready = buffer.take_paste_ready().expect("paste ready");
        assert_eq!(ready.paste_text, "BridgeMind, Thanks for the review.");
        assert_eq!(ready.paste_text, context.prepare_text(&ready.raw_text));
        assert!(ready.raw_text.contains("bridge mind"));
        // Reported for display and metrics only; release requires a complete Final.
        assert_eq!(ready.covered_through_ms, 1500);
    }

    #[test]
    fn captured_text_policy_stays_frozen_until_the_next_session() {
        let buffer = LivePasteBuffer::new();
        let mut source = sample_context();
        let frozen = source.clone();
        buffer.begin_session("s1", frozen.clone());

        source.cleanup_enabled = false;
        source.dictionary[0].replacement = "ChangedName".into();
        source.snippets[0].body = "Replacement body.".into();
        let raw = "  um bridge mind comma ship intro  ";
        let expected = "BridgeMind, Thanks for the review.";
        let preview = buffer.apply_partial("s1", raw, 1_000, None, 1).unwrap();
        assert_eq!(preview.stable_text, expected);
        assert_eq!(frozen.prepare_text(raw), expected);

        buffer.begin_session("s2", source.clone());
        let preview = buffer.apply_partial("s2", raw, 1_000, None, 1).unwrap();
        assert_eq!(preview.stable_text, raw.trim());
        assert_eq!(source.prepare_text(raw), raw.trim());

        source.cleanup_enabled = true;
        buffer.begin_session("s3", source.clone());
        let preview = buffer.apply_partial("s3", raw, 1_000, None, 1).unwrap();
        assert_eq!(preview.stable_text, "ChangedName, Replacement body.");
        assert_eq!(source.prepare_text(raw), preview.stable_text);
        assert_eq!(frozen.prepare_text(raw), expected);
    }

    #[test]
    fn repeated_hypotheses_preserve_coverage_and_reset_context() {
        let buffer = LivePasteBuffer::new();
        buffer.begin_session("session", sample_context());
        let raw = "um bridge mind comma ship intro. ".repeat(256);
        let first = buffer
            .apply_partial("session", &raw, 1_000, Some(40), 1)
            .unwrap();
        let started = std::time::Instant::now();
        for revision in 2..=65 {
            let event = buffer
                .apply_partial("session", &raw, revision * 1_000, Some(40), revision)
                .unwrap();
            assert_eq!(event.stable_text, first.stable_text);
            assert_eq!(event.covered_through_ms, revision * 1_000);
            assert_eq!(event.revision, revision);
            assert_eq!(event.first_partial_latency_ms, Some(40));
        }
        eprintln!(
            "64 unchanged previews, {} input bytes: {:.3} ms",
            raw.len(),
            started.elapsed().as_secs_f64() * 1_000.0
        );
        let checkpoint = buffer.take_paste_ready().unwrap();
        assert_eq!(checkpoint.covered_through_ms, 65_000);
        assert_eq!(checkpoint.paste_text, first.stable_text);
        assert!(checkpoint.raw_text.starts_with("um bridge mind"));
        buffer.begin_session(
            "session",
            DictationTextContext {
                cleanup_enabled: false,
                dictionary: Vec::new(),
                snippets: Vec::new(),
            },
        );
        let reset = buffer
            .apply_partial("session", &raw, 1_000, None, 1)
            .unwrap();
        assert_eq!(reset.stable_text, raw.trim());
        assert_ne!(reset.stable_text, first.stable_text);
        let changed = buffer
            .apply_partial("session", "different text", 2_000, None, 2)
            .unwrap();
        assert_eq!(changed.stable_text, "different text");
    }

    #[test]
    fn empty_buffer_is_not_paste_ready() {
        let buffer = LivePasteBuffer::new();
        buffer.begin_session("s1", sample_context());
        assert!(buffer.take_paste_ready().is_none());
    }

    #[test]
    fn ignores_events_for_other_sessions() {
        let buffer = LivePasteBuffer::new();
        buffer.begin_session("s1", sample_context());
        assert!(buffer.apply_partial("other", "hello", 0, None, 1).is_none());
    }

    #[test]
    fn ignores_stale_partial_without_replacing_newer_text() {
        let buffer = LivePasteBuffer::new();
        buffer.begin_session("s1", sample_context());
        buffer
            .apply_partial("s1", "newer", 1_000, None, 4)
            .expect("newer event");
        assert!(buffer.apply_partial("s1", "older", 900, None, 3).is_none());
        assert_eq!(buffer.take_paste_ready().unwrap().paste_text, "Newer.");
    }

    #[test]
    fn ignores_stale_stable_segment_without_moving_coverage_backwards() {
        let buffer = LivePasteBuffer::new();
        buffer.begin_session("s1", sample_context());
        buffer
            .apply_stable("s1", "newer", 1_000, None)
            .expect("newer segment");
        assert!(buffer.apply_stable("s1", "older", 900, None).is_none());
        let ready = buffer.take_paste_ready().expect("paste ready");
        assert_eq!(ready.paste_text, "Newer.");
        assert_eq!(ready.covered_through_ms, 1_000);
    }
}
