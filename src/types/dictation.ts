export type DictationMode = "toggle" | "pushToTalk";

export interface AppSettings {
  hotkey: string;
  mode: DictationMode;
  microphoneName: string | null;
  restoreClipboard: boolean;
  autoInject: boolean;
  cleanupEnabled: boolean;
  startAtLogin: boolean;
  onboardingComplete: boolean;
  onboardingVersion: string;
  advancedRuntimeEnabled: boolean;
  advancedModelPath: string;
  advancedWhisperCliPath: string;
}

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
  createdAt: string;
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
  message: string;
}

export interface DictationResult {
  session: TranscriptSession;
  injection: InjectionResult | null;
}

export interface MicrophoneInfo {
  name: string;
  isDefault: boolean;
}

export interface ModelStatus {
  whisperCliFound: boolean;
  modelFound: boolean;
  ready: boolean;
  message: string;
  source: "bundled" | "advancedOverride";
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
  | "advanced"
  | "settings";

export interface AppNotice {
  tone: "neutral" | "success" | "warning" | "error";
  message: string;
}
