pub use atmospeak_asr_protocol::{AsrBackend, StreamingMetrics, TranscriptionProfile};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub hotkey: String,
    pub mode: DictationMode,
    pub microphone_name: Option<String>,
    pub restore_clipboard: bool,
    pub auto_inject: bool,
    pub cleanup_enabled: bool,
    pub start_at_login: bool,
    pub transcript_retention_days: u32,
    pub onboarding_complete: bool,
    pub onboarding_version: String,
    pub audio_calibration: Option<AudioCalibrationRecord>,
    pub active_model_id: String,
    pub advanced_runtime_enabled: bool,
    pub advanced_model_path: String,
    pub advanced_whisper_cli_path: String,
    // Companion appearance. Phase A locked settings to the 12 fields above; these
    // five were promoted from the design prototype's tweak panel into real,
    // persisted settings. Container-level `serde(default)` keeps older setting
    // blobs loadable.
    pub accent: Accent,
    pub dock_shape: DockShape,
    pub wave_style: WaveStyle,
    pub dock_theme: DockTheme,
    pub motion: Motion,
    pub model_selection_mode: ModelSelectionMode,
    pub transcription_profile: TranscriptionProfile,
    pub acceleration_preference: AccelerationPreference,
    pub live_preview_enabled: bool,
    /// When true, cleaned text is optionally rewritten by an LLM before paste.
    pub auto_polish: bool,
    pub polish_style: PolishStyle,
    pub custom_instructions: String,
    pub polish_endpoint: String,
    pub polish_model: String,
    pub polish_provider: PolishProvider,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PolishStyle {
    #[default]
    None,
    Concise,
    Formal,
    Casual,
    Excited,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PolishProvider {
    /// Bundled llama-server + curated GGUF (default, frictionless).
    #[default]
    Bundled,
    Ollama,
    OpenaiCompatible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelSelectionMode {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccelerationPreference {
    Auto,
    Vulkan,
    Cpu,
}

/// Companion pigment. Drives `--accent*` and the neon listening glow.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Accent {
    Dusk,
    Teal,
    Lilac,
}

/// Resting silhouette of the dock. It always morphs to a capsule while active.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DockShape {
    Orb,
    Capsule,
    Tape,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WaveStyle {
    Ribbon,
    Bars,
    Pulse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DockTheme {
    Dark,
    Light,
}

/// Animation tempo. `Calm` lengthens the breath cycle for a quieter companion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Motion {
    Lively,
    Calm,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Win".to_string(),
            mode: DictationMode::PushToTalk,
            microphone_name: None,
            restore_clipboard: true,
            auto_inject: true,
            cleanup_enabled: true,
            start_at_login: false,
            transcript_retention_days: 0,
            onboarding_complete: false,
            onboarding_version: String::new(),
            audio_calibration: None,
            active_model_id: "base.en".to_string(),
            advanced_runtime_enabled: false,
            advanced_model_path: String::new(),
            advanced_whisper_cli_path: String::new(),
            accent: Accent::Dusk,
            dock_shape: DockShape::Orb,
            wave_style: WaveStyle::Ribbon,
            dock_theme: DockTheme::Dark,
            motion: Motion::Lively,
            model_selection_mode: ModelSelectionMode::Automatic,
            transcription_profile: TranscriptionProfile::Balanced,
            acceleration_preference: AccelerationPreference::Auto,
            live_preview_enabled: true,
            auto_polish: false,
            polish_style: PolishStyle::None,
            custom_instructions: String::new(),
            polish_endpoint: "http://127.0.0.1:11434/v1/chat/completions".to_string(),
            polish_model: "qwen2.5-0.5b".to_string(),
            polish_provider: PolishProvider::Bundled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub registered: bool,
    pub hotkey: String,
    pub paused: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutSource {
    RegisteredHotkey,
    RawInput,
    LowLevelHook,
    Tray,
    Overlay,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutSignal {
    Pressed,
    Released,
    Toggle,
    Cancel,
    Start,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutGesture {
    pub gesture_id: u64,
    pub registration_generation: u64,
    pub signal: ShortcutSignal,
    pub source: ShortcutSource,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineActionAck {
    pub gesture_id: u64,
    pub accepted: bool,
    pub state_before: String,
    pub state_after: String,
    pub reason: Option<String>,
    pub acknowledged_at_ms: u64,
}

impl Default for ShortcutStatus {
    fn default() -> Self {
        Self {
            registered: false,
            hotkey: String::new(),
            paused: false,
            message: "Global shortcut has not been registered yet.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DictationMode {
    Toggle,
    PushToTalk,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DictationPhase {
    Idle,
    Listening,
    Finalizing,
    Pasted,
    Saved,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveTranscriptEvent {
    pub session_id: String,
    pub revision: u64,
    pub stable_text: String,
    pub partial_text: String,
    pub covered_through_ms: u64,
    pub first_partial_latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub phrase: String,
    pub replacement: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    pub trigger: String,
    pub body: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSession {
    pub id: String,
    pub raw_text: String,
    pub cleaned_text: String,
    /// LLM polish result when auto-polish succeeded. Kept for Undo/Redo AI edit.
    #[serde(default)]
    pub polished_text: Option<String>,
    /// When true and `polished_text` is set, UI / paste-again prefer the polished text.
    #[serde(default = "default_prefer_polished")]
    pub prefer_polished: bool,
    pub audio_path: String,
    pub duration_ms: u64,
    pub word_count: usize,
    pub injected: bool,
    pub source_application: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn default_prefer_polished() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStats {
    pub total_sessions: usize,
    pub total_words: usize,
    pub total_duration_ms: u64,
    pub average_words_per_minute: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub dictionary: Vec<DictionaryEntry>,
    pub snippets: Vec<Snippet>,
    pub sessions: Vec<TranscriptSession>,
    pub stats: DictationStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStarted {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub microphone_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationResult {
    pub session: TranscriptSession,
    pub injection: Option<InjectionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvent {
    pub created_at: DateTime<Utc>,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectionResult {
    pub injected: bool,
    pub restored_clipboard: bool,
    pub restored_target: bool,
    pub target_process_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneInfo {
    pub name: String,
    pub is_default: bool,
    pub is_selected: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicLevel {
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub noise_floor_dbfs: f32,
    pub clipping_ratio: f32,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCalibrationRecord {
    pub device_name: String,
    pub checked_at: DateTime<Utc>,
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub snr_db: f32,
    pub model_id: String,
    pub asr_backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundCheckResult {
    pub passed: bool,
    pub failure_code: Option<String>,
    pub device_name: String,
    pub capture_format: String,
    pub duration_ms: u64,
    pub active_speech_ms: u64,
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub noise_floor_dbfs: f32,
    pub snr_db: f32,
    pub clipping_ratio: f32,
    pub transcript: String,
    pub expected_phrase: String,
    pub token_similarity: f32,
    pub asr_backend: String,
    pub model_id: String,
    pub capture_ms: u64,
    pub asr_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub whisper_cli_found: bool,
    pub model_found: bool,
    pub ready: bool,
    pub message: String,
    pub source: RuntimeSource,
    pub whisper_cli_path: String,
    pub model_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSource {
    Bundled,
    ManagedModel,
    AdvancedOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInventory {
    pub active_model_id: String,
    pub models: Vec<ModelInventoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInventoryItem {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub bundled: bool,
    pub path: Option<String>,
    pub size_mb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadProgress {
    pub model_id: String,
    pub status: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageMetrics {
    pub session_id: String,
    /// Wall time to stop stream and take sample buffer ownership (not hold duration).
    pub capture_stop_ms: u64,
    pub write_ms: u64,
    pub asr_ms: u64,
    pub cleanup_ms: u64,
    pub inject_ms: u64,
    pub total_ms: u64,
    pub asr_backend: String,
    pub audio_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDictationEvent {
    pub recording: Option<RecordingStarted>,
    pub phase: DictationPhase,
    pub message: String,
    pub result: Option<DictationResult>,
    pub metrics: Option<StageMetrics>,
}
