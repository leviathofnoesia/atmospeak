use parking_lot::Mutex;

use crate::models::{DictionaryEntry, LiveTranscriptEvent, Snippet};
use crate::services::cleanup;

/// Dictionary / snippet snapshot frozen at listen start so live cleanup matches paste.
#[derive(Debug, Clone)]
pub struct LivePasteContext {
    pub cleanup_enabled: bool,
    pub dictionary: Vec<DictionaryEntry>,
    pub snippets: Vec<Snippet>,
}

/// Paste-ready hypothesis built during listening from stable + partial ASR events.
#[derive(Debug, Clone)]
pub struct LivePasteSnapshot {
    pub session_id: String,
    pub raw_text: String,
    pub paste_text: String,
    pub covered_through_ms: u64,
}

/// Preview decode lag budget: rolling previews update at most ~1/s, so a short
/// uncovered tail is expected. Larger gaps fall through to Final.
pub const PREVIEW_COVERAGE_SLACK_MS: u64 = 1200;

impl LivePasteSnapshot {
    /// True when the live hypothesis covers nearly all captured audio.
    pub fn covers_duration(&self, duration_ms: u64) -> bool {
        self.covered_through_ms
            .saturating_add(PREVIEW_COVERAGE_SLACK_MS)
            >= duration_ms
    }
}

#[derive(Debug, Default)]
struct LivePasteState {
    context: Option<LivePasteContext>,
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

    pub fn begin_session(&self, session_id: impl Into<String>, context: LivePasteContext) {
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

    /// Snapshot for aggressive paste when the cleaned hypothesis is non-empty.
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
        state.partial_text = text.to_string();
        state.revision = state.revision.saturating_add(1).max(revision_hint);
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
    format!("{stable} {partial}").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn recompute_and_event(
    state: &mut LivePasteState,
    covered_through_ms: u64,
    first_partial_latency_ms: Option<u64>,
) -> LiveTranscriptEvent {
    let raw = join_raw(&state.stable_text, &state.partial_text);
    state.raw_text = raw.clone();
    state.covered_through_ms = covered_through_ms;
    let paste = match state.context.as_ref() {
        Some(context) if context.cleanup_enabled => {
            cleanup::clean_text(&raw, &context.dictionary, &context.snippets)
        }
        Some(_) => raw.trim().to_string(),
        None => raw.trim().to_string(),
    };
    state.paste_text = paste.clone();
    // Emit the full paste-ready string as stable so the orb matches what release pastes.
    LiveTranscriptEvent {
        session_id: state.session_id.clone(),
        revision: state.revision,
        stable_text: paste,
        partial_text: String::new(),
        covered_through_ms,
        first_partial_latency_ms,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn sample_context() -> LivePasteContext {
        LivePasteContext {
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
        buffer.begin_session("s1", sample_context());

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
        assert!(ready.raw_text.contains("bridge mind"));
        assert_eq!(ready.covered_through_ms, 1500);
        assert!(ready.covers_duration(2000));
        assert!(!ready.covers_duration(4000));
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
}
