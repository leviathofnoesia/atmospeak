export type DictationMode = "toggle" | "pushToTalk";

export interface AppSettings {
  hotkey: string;
  mode: DictationMode;
  microphoneName: string | null;
  restoreClipboard: boolean;
  autoInject: boolean;
  cleanupEnabled: boolean;
  startAtLogin: boolean;
  transcriptRetentionDays: number;
  onboardingComplete: boolean;
  onboardingVersion: string;
  audioCalibration: AudioCalibrationRecord | null;
  activeModelId: string;
  advancedRuntimeEnabled: boolean;
  advancedModelPath: string;
  advancedWhisperCliPath: string;
  /** Companion appearance — mirrors the Rust `AppSettings` appearance block. */
  accent: Accent;
  dockShape: DockShape;
  waveStyle: WaveStyle;
  dockTheme: DockTheme;
  motion: Motion;
}

export type Accent = "dusk" | "teal" | "lilac";
export type DockShape = "orb" | "capsule" | "tape";
export type WaveStyle = "ribbon" | "bars" | "pulse";
export type DockTheme = "dark" | "light";
export type Motion = "lively" | "calm";

export interface DictionaryEntry {
  id: string;
  phrase: string;
  replacement: string;
  enabled: boolean;
  createdAt: string;
}

export interface Snippet {
  id: string;
  trigger: string;
  body: string;
  enabled: boolean;
  createdAt: string;
}

export interface TranscriptSession {
  id: string;
  rawText: string;
  cleanedText: string;
  audioPath: string;
  durationMs: number;
  wordCount: number;
  injected: boolean;
  sourceApplication: string | null;
  createdAt: string;
}

export interface RuntimeEvent {
  kind: string;
  message: string;
  createdAt: string;
}

export type DictationPhase = "idle" | "listening" | "processing" | "pasted" | "error";

export interface StageMetrics {
  sessionId: string;
  captureStopMs: number;
  writeMs: number;
  asrMs: number;
  cleanupMs: number;
  injectMs: number;
  totalMs: number;
  asrBackend: string;
  audioDurationMs: number;
}

export interface NativeDictationEvent {
  recording: RecordingStarted | null;
  phase: DictationPhase;
  message: string;
  result: DictationResult | null;
  metrics: StageMetrics | null;
}

export interface DictationStats {
  totalSessions: number;
  totalWords: number;
  totalDurationMs: number;
  averageWordsPerMinute: number;
}

export interface AppSnapshot {
  settings: AppSettings;
  dictionary: DictionaryEntry[];
  snippets: Snippet[];
  sessions: TranscriptSession[];
  stats: DictationStats;
}

export interface ShortcutStatus {
  registered: boolean;
  hotkey: string;
  paused: boolean;
  message: string;
}

export interface RecordingStarted {
  id: string;
  startedAt: string;
  microphoneName: string;
}

export interface InjectionResult {
  injected: boolean;
  restoredClipboard: boolean;
  restoredTarget: boolean;
  targetProcessName: string | null;
  message: string;
}

export interface DictationResult {
  session: TranscriptSession;
  injection: InjectionResult | null;
}

export interface MicrophoneInfo {
  name: string;
  isDefault: boolean;
  isSelected: boolean;
  available: boolean;
}

export interface ShortcutKeyEvent {
  code: number;
  key: string;
  pressed: boolean;
}

export interface ShortcutCaptureEvent {
  keys: string[];
  completed: string | null;
  error: string | null;
  timestampMs: number;
}

export interface MicLevel {
  rmsDbfs: number;
  peakDbfs: number;
  noiseFloorDbfs: number;
  clippingRatio: number;
  timestampMs: number;
}

export interface AudioCalibrationRecord {
  deviceName: string;
  checkedAt: string;
  rmsDbfs: number;
  peakDbfs: number;
  snrDb: number;
  modelId: string;
  asrBackend: string;
}

export interface SoundCheckResult {
  passed: boolean;
  failureCode: string | null;
  deviceName: string;
  captureFormat: string;
  durationMs: number;
  activeSpeechMs: number;
  rmsDbfs: number;
  peakDbfs: number;
  noiseFloorDbfs: number;
  snrDb: number;
  clippingRatio: number;
  transcript: string;
  expectedPhrase: string;
  tokenSimilarity: number;
  asrBackend: string;
  modelId: string;
  captureMs: number;
  asrMs: number;
  totalMs: number;
}

export interface ModelStatus {
  whisperCliFound: boolean;
  modelFound: boolean;
  ready: boolean;
  message: string;
  source: "bundled" | "managedModel" | "advancedOverride";
  whisperCliPath: string;
  modelPath: string;
}

export interface ModelInventory {
  activeModelId: string;
  models: ModelInventoryItem[];
}

export interface ModelInventoryItem {
  id: string;
  label: string;
  installed: boolean;
  bundled: boolean;
  path: string | null;
  sizeMb: number | null;
}

export interface ModelDownloadProgress {
  modelId: string;
  status: "starting" | "downloading" | "verifying" | "installed" | "cancelled" | "error";
  bytesDownloaded: number;
  totalBytes: number | null;
  percent: number | null;
  message: string;
}

export type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "current"
  | "downloading"
  | "readyToRelaunch"
  | "error";

export interface UpdateCheckResult {
  available: boolean;
  currentVersion: string;
  version: string | null;
  date: string | null;
  body: string | null;
  message: string;
}

export interface ReleaseArtifact {
  id: string;
  label: string;
  fileName: string;
  kind: "installer" | "msi" | "portable" | "metadata" | "checksum";
  url: string;
  recommended: boolean;
}

export interface DownloadArtifact extends ReleaseArtifact {
  sizeBytes: number | null;
  sha256: string | null;
}

export type HubTab =
  | "home"
  | "history"
  | "dictionary"
  | "snippets"
  | "settings";

export interface AppNotice {
  tone: "neutral" | "success" | "warning" | "error";
  message: string;
}

// "warning" is allowed for soft UX notices (e.g. shortcuts paused).

/** Shared onboarding contract — must match Rust `ONBOARDING_VERSION`. */
export const ONBOARDING_VERSION = "atmospeak-setup-v2";

export function defaultSettings(): AppSettings {
  return {
    hotkey: "Ctrl+Win",
    mode: "pushToTalk",
    microphoneName: null,
    restoreClipboard: true,
    autoInject: true,
    cleanupEnabled: true,
    startAtLogin: false,
    transcriptRetentionDays: 0,
    onboardingComplete: false,
    onboardingVersion: "",
    audioCalibration: null,
    activeModelId: "base.en",
    advancedRuntimeEnabled: false,
    advancedModelPath: "",
    advancedWhisperCliPath: "",
    accent: "dusk",
    dockShape: "orb",
    waveStyle: "ribbon",
    dockTheme: "dark",
    motion: "lively",
  };
}
