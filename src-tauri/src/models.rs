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
    pub onboarding_complete: bool,
    pub onboarding_version: String,
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
            onboarding_complete: false,
            onboarding_version: String::new(),
            advanced_runtime_enabled: false,
            advanced_model_path: String::new(),
            advanced_whisper_cli_path: String::new(),
            accent: Accent::Dusk,
            dock_shape: DockShape::Orb,
            wave_style: WaveStyle::Ribbon,
            dock_theme: DockTheme::Dark,
            motion: Motion::Lively,
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
    Processing,
    Pasted,
    Error,
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
    pub audio_path: String,
    pub duration_ms: u64,
    pub word_count: usize,
    pub injected: bool,
    pub created_at: DateTime<Utc>,
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
