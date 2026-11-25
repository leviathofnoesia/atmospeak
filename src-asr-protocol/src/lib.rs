use serde::{Deserialize, Serialize};

pub mod speech_models;

/// Version 5 reports model-specific streaming support and routes language hints.
pub const PROTOCOL_VERSION: u16 = 5;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

pub const PARAKEET_MODEL_ID: &str = "parakeet-unified-en";
pub const PARAKEET_MODEL_FILE: &str = "parakeet-unified-en-0.6b-Q8_0.gguf";
pub const PARAKEET_MODEL_BYTES: u64 = 731_357_568;
pub const PARAKEET_MODEL_SHA256: &str =
    "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AsrBackend {
    Vulkan,
    Cpu,
    Host,
    Cli,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptionProfile {
    Balanced,
    Quality,
    Speed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "command", content = "payload")]
pub enum AsrCommand {
    Hello {
        protocol_version: u16,
    },
    LoadModel {
        model_path: String,
        backend: AsrBackend,
        threads: u16,
    },
    StartSession {
        session_id: String,
        language: String,
        initial_prompt: String,
        profile: TranscriptionProfile,
    },
    AudioFrame {
        session_id: String,
        sequence: u64,
        timestamp_ms: u64,
        pcm_s16le: Vec<u8>,
    },
    StopSession {
        session_id: String,
        /// Highest audio sequence the client attempted before stopping.
        ///
        /// The value is intentionally based on attempted frames rather than
        /// only frames accepted by the bounded writer queue: the host uses it
        /// to account for a dropped tail when no later frame can expose a
        /// sequence gap.
        #[serde(default)]
        last_sequence: Option<u64>,
    },
    CancelSession {
        session_id: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsrCapabilities {
    pub protocol_version: u16,
    pub backend: AsrBackend,
    pub streaming: bool,
    pub vad: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingMetrics {
    pub session_id: String,
    pub backend: AsrBackend,
    pub model_id: String,
    pub first_partial_ms: Option<u64>,
    pub stop_ack_ms: u64,
    pub finalize_ms: u64,
    pub paste_ms: u64,
    pub processed_during_recording_ms: u64,
    pub tail_audio_ms: u64,
    pub max_backlog_ms: u64,
    pub audio_frames_dropped: u64,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "event", content = "payload")]
pub enum AsrEvent {
    Ready {
        capabilities: AsrCapabilities,
    },
    SpeechState {
        session_id: String,
        active: bool,
    },
    Partial {
        session_id: String,
        revision: u64,
        text: String,
        covered_through_ms: u64,
    },
    StableSegment {
        session_id: String,
        index: u32,
        text: String,
        start_ms: u64,
        end_ms: u64,
    },
    Final {
        session_id: String,
        text: String,
        processed_during_recording_ms: u64,
        tail_audio_ms: u64,
    },
    Metrics(StreamingMetrics),
    Error {
        session_id: Option<String>,
        recoverable: bool,
        message: String,
    },
}
