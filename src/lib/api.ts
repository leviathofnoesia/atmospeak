import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import type {
  AppSettings,
  AppSnapshot,
  DictationResult,
  DictionaryEntry,
  DownloadArtifact,
  ExportFormat,
  FeedbackResult,
  HistorySearchFilters,
  InjectionResult,
  MicrophoneInfo,
  ModelInventory,
  ModelStatus,
  PolishResult,
  RecentAppUsage,
  RecordingStarted,
  ReleaseArtifact,
  RuntimeEvent,
  ShortcutStatus,
  Snippet,
  TranscriptSession,
  UpdateCheckResult,
} from "../types/dictation";

type InvokeArgs = Record<string, unknown>;

interface WindowWithTauri extends Window {
  __TAURI_INTERNALS__?: unknown;
}

const releaseBaseUrl = "https://github.com/leviathofnoesia/atmospeak/releases/latest/download";
const releaseVersion = "0.1.9";

const defaultSettings: AppSettings = {
  hotkey: "Ctrl+Win",
  mode: "pushToTalk",
  microphoneName: null,
  restoreClipboard: true,
  autoInject: true,
  cleanupEnabled: true,
  startAtLogin: false,
  onboardingComplete: false,
  onboardingVersion: "",
  advancedRuntimeEnabled: false,
  advancedModelPath: "",
  advancedWhisperCliPath: "",
  activeModelId: "base.en",
  language: null,
  injectionMode: "auto",
  customInstructions: "",
  autoPolish: false,
  polishStyle: "concise",
  polishProvider: "ollama",
  polishEndpoint: "http://127.0.0.1:11434/api/chat",
  polishModel: "llama3.2",
  livePreviewEnabled: true,
  livePreviewIntervalMs: 750,
  finalPassEnabled: true,
  privacyMode: false,
  autoDeleteTranscriptsAfterMinutes: null,
  bubbleSize: "medium",
  bubbleOpacity: 1,
  feedbackWebhookUrl: "",
};

let mockSnapshot: AppSnapshot = {
  settings: defaultSettings,
  dictionary: [
    {
      id: "mock-dictionary-1",
      phrase: "wind speak",
      replacement: "Atmospeak",
      enabled: true,
      createdAt: new Date().toISOString(),
    },
  ],
  snippets: [
    {
      id: "mock-snippet-1",
      trigger: "ship note",
      body: "Thanks for the review. I pushed the local dictation prototype.",
      enabled: true,
      createdAt: new Date().toISOString(),
    },
  ],
  sessions: [],
  stats: {
    totalSessions: 0,
    totalWords: 0,
    totalDurationMs: 0,
    averageWordsPerMinute: 0,
  },
};

export function hasTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in (window as WindowWithTauri);
}

function releaseArtifacts(): ReleaseArtifact[] {
  return [
    {
      id: "nsis",
      label: "Windows installer",
      fileName: `atmospeak_${releaseVersion}_x64-setup.exe`,
      kind: "installer",
      url: `${releaseBaseUrl}/atmospeak_${releaseVersion}_x64-setup.exe`,
      recommended: true,
    },
    {
      id: "msi",
      label: "Windows MSI",
      fileName: `atmospeak_${releaseVersion}_x64_en-US.msi`,
      kind: "msi",
      url: `${releaseBaseUrl}/atmospeak_${releaseVersion}_x64_en-US.msi`,
      recommended: false,
    },
    {
      id: "portable",
      label: "Portable zip",
      fileName: `atmospeak_${releaseVersion}_x64-portable.zip`,
      kind: "portable",
      url: `${releaseBaseUrl}/atmospeak_${releaseVersion}_x64-portable.zip`,
      recommended: false,
    },
    {
      id: "checksums",
      label: "Checksums",
      fileName: "SHA256SUMS.txt",
      kind: "checksum",
      url: `${releaseBaseUrl}/SHA256SUMS.txt`,
      recommended: false,
    },
  ];
}

function command<T>(name: string, args: InvokeArgs | undefined, fallback: () => T | Promise<T>) {
  if (hasTauriRuntime()) {
    return invoke<T>(name, args);
  }

  return Promise.resolve(fallback());
}

function recalculateStats(snapshot: AppSnapshot): AppSnapshot {
  const totalWords = snapshot.sessions.reduce((sum, session) => sum + session.wordCount, 0);
  const totalDurationMs = snapshot.sessions.reduce((sum, session) => sum + session.durationMs, 0);
  return {
    ...snapshot,
    stats: {
      totalSessions: snapshot.sessions.length,
      totalWords,
      totalDurationMs,
      averageWordsPerMinute:
        totalDurationMs === 0 ? 0 : totalWords / (totalDurationMs / 60_000),
    },
  };
}

export function getAppSnapshot(): Promise<AppSnapshot> {
  return command("get_app_snapshot", undefined, () => mockSnapshot);
}

export function getShortcutStatus(): Promise<ShortcutStatus> {
  return command("get_shortcut_status", undefined, () => ({
    registered: true,
    hotkey: mockSnapshot.settings.hotkey,
    paused: false,
    message: `Global shortcut registered: ${mockSnapshot.settings.hotkey}`,
  }));
}

export function setShortcutsPaused(paused: boolean): Promise<ShortcutStatus> {
  return command("set_shortcuts_paused", { paused }, () => ({
    registered: true,
    hotkey: mockSnapshot.settings.hotkey,
    paused,
    message: paused
      ? "Global shortcuts paused. Use the floating control, tray, or resume shortcuts."
      : `Global shortcut registered: ${mockSnapshot.settings.hotkey}.`,
  }));
}

export function showFloatingControl(): Promise<void> {
  return command("show_overlay_window", undefined, () => undefined);
}

export function getRecordingLevel(): Promise<number> {
  return command("get_recording_level", undefined, () => 0.62);
}

export function listMicrophones(): Promise<MicrophoneInfo[]> {
  return command("list_microphones", undefined, () => [
    { name: "System default microphone", isDefault: true },
    { name: "USB-C Studio Mic", isDefault: false },
  ]);
}

export function saveSettings(settings: AppSettings): Promise<AppSnapshot> {
  return command("save_settings", { settings }, () => {
    mockSnapshot = { ...mockSnapshot, settings };
    return mockSnapshot;
  });
}

export function startRecording(): Promise<RecordingStarted> {
  return command("start_recording", undefined, () => ({
    id: crypto.randomUUID(),
    startedAt: new Date().toISOString(),
    microphoneName: mockSnapshot.settings.microphoneName ?? "System default microphone",
  }));
}

export function stopRecording(): Promise<DictationResult> {
  return command("stop_recording", undefined, () => {
    const cleanedText =
      "Atmospeak captured this offline prototype transcript and pasted it into the active window.";
    const session: TranscriptSession = {
      id: crypto.randomUUID(),
      rawText: cleanedText,
      cleanedText,
      audioPath: "mock://recording.wav",
      durationMs: 4200,
      wordCount: cleanedText.split(/\s+/).length,
      injected: mockSnapshot.settings.autoInject,
      createdAt: new Date().toISOString(),
      appName: "Letters",
      notes: "",
    };
    mockSnapshot = recalculateStats({
      ...mockSnapshot,
      sessions: [session, ...mockSnapshot.sessions],
    });
    return {
      session,
      injection: mockSnapshot.settings.autoInject
        ? {
            injected: true,
            restoredClipboard: mockSnapshot.settings.restoreClipboard,
            message: "Mock transcript pasted into the focused application.",
          }
        : null,
    };
  });
}

export function cancelRecording(): Promise<void> {
  return command("cancel_recording", undefined, () => undefined);
}

export function injectText(text: string): Promise<InjectionResult> {
  return command("inject_text", { text }, () => ({
    injected: text.trim().length > 0,
    restoredClipboard: mockSnapshot.settings.restoreClipboard,
    message: "Mock transcript copied to the focused application.",
  }));
}

export async function copyText(text: string): Promise<string> {
  if (text.trim().length === 0) {
    throw new Error("cannot copy an empty transcript");
  }

  if (hasTauriRuntime()) {
    await writeText(text);
    return "Transcript copied to clipboard.";
  }

  return "Transcript copied to clipboard.";
}

export function upsertDictionaryEntry(entry: DictionaryEntry): Promise<AppSnapshot> {
  return command("upsert_dictionary_entry", { entry }, () => {
    const saved = { ...entry, id: entry.id || crypto.randomUUID() };
    mockSnapshot = {
      ...mockSnapshot,
      dictionary: [
        saved,
        ...mockSnapshot.dictionary.filter((candidate) => candidate.id !== saved.id),
      ],
    };
    return mockSnapshot;
  });
}

export function deleteDictionaryEntry(id: string): Promise<AppSnapshot> {
  return command("delete_dictionary_entry", { id }, () => {
    mockSnapshot = {
      ...mockSnapshot,
      dictionary: mockSnapshot.dictionary.filter((entry) => entry.id !== id),
    };
    return mockSnapshot;
  });
}

export function upsertSnippet(snippet: Snippet): Promise<AppSnapshot> {
  return command("upsert_snippet", { snippet }, () => {
    const saved = { ...snippet, id: snippet.id || crypto.randomUUID() };
    mockSnapshot = {
      ...mockSnapshot,
      snippets: [saved, ...mockSnapshot.snippets.filter((candidate) => candidate.id !== saved.id)],
    };
    return mockSnapshot;
  });
}

export function deleteSnippet(id: string): Promise<AppSnapshot> {
  return command("delete_snippet", { id }, () => {
    mockSnapshot = {
      ...mockSnapshot,
      snippets: mockSnapshot.snippets.filter((snippet) => snippet.id !== id),
    };
    return mockSnapshot;
  });
}

export function getModelStatus(): Promise<ModelStatus> {
  return command("get_model_status", undefined, () => {
    const usingAdvanced = mockSnapshot.settings.advancedRuntimeEnabled;
    const whisperCliFound = usingAdvanced
      ? mockSnapshot.settings.advancedWhisperCliPath.trim().length > 0
      : true;
    const modelFound = usingAdvanced ? mockSnapshot.settings.advancedModelPath.trim().length > 0 : true;
    return {
      whisperCliFound,
      modelFound,
      ready: whisperCliFound && modelFound,
      source: usingAdvanced ? "advancedOverride" : "bundled",
      whisperCliPath: usingAdvanced
        ? mockSnapshot.settings.advancedWhisperCliPath
        : "app://resources/whisper-runtime/whisper-cli.exe",
      modelPath: usingAdvanced
        ? mockSnapshot.settings.advancedModelPath
        : "app://resources/models/ggml-base.en.bin",
      message:
        whisperCliFound && modelFound
          ? "Bundled offline transcription runtime is ready."
          : "The advanced transcription runtime is incomplete.",
    };
  });
}

export function getModelInventory(): Promise<ModelInventory> {
  return command("get_model_inventory", undefined, () => ({
    activeModelId: mockSnapshot.settings.advancedRuntimeEnabled ? "advanced-override" : "base.en",
    models: [
      {
        id: "base.en",
        label: "Base English",
        installed: true,
        bundled: true,
        path: "app://resources/models/ggml-base.en.bin",
        sizeMb: 142,
      },
      {
        id: "tiny.en",
        label: "Tiny English",
        installed: false,
        bundled: false,
        path: null,
        sizeMb: null,
      },
      {
        id: "small.en",
        label: "Small English",
        installed: false,
        bundled: false,
        path: null,
        sizeMb: null,
      },
      {
        id: "medium.en",
        label: "Medium English",
        installed: false,
        bundled: false,
        path: null,
        sizeMb: null,
      },
    ],
  }));
}

export async function checkForUpdates(): Promise<UpdateCheckResult> {
  if (!hasTauriRuntime()) {
    return {
      available: false,
      currentVersion: "0.1.9",
      version: null,
      date: null,
      body: null,
      message: "Atmospeak is current in browser preview mode.",
    };
  }

  const currentVersion = await getVersion();
  const update = await check();
  if (!update) {
    return {
      available: false,
      currentVersion,
      version: null,
      date: null,
      body: null,
      message: "Atmospeak is up to date.",
    };
  }

  return {
    available: update.available,
    currentVersion,
    version: update.version,
    date: update.date ?? null,
    body: update.body ?? null,
    message: update.available
      ? `Atmospeak ${update.version} is ready to install.`
      : "Atmospeak is up to date.",
  };
}

export async function downloadAndInstallUpdate(): Promise<UpdateCheckResult> {
  if (!hasTauriRuntime()) {
    return {
      available: false,
      currentVersion: "0.1.9",
      version: null,
      date: null,
      body: null,
      message: "Mock update flow completed.",
    };
  }

  const currentVersion = await getVersion();
  const update = await check();
  if (!update?.available) {
    return {
      available: false,
      currentVersion,
      version: null,
      date: null,
      body: null,
      message: "Atmospeak is already up to date.",
    };
  }

  await update.downloadAndInstall();
  await relaunch();
  return {
    available: false,
    currentVersion,
    version: update.version,
    date: update.date ?? null,
    body: update.body ?? null,
    message: `Atmospeak ${update.version} installed. Relaunching.`,
  };
}

export function getRecordingFftBands(): Promise<number[]> {
  return command("get_recording_fft_bands", undefined, () =>
    Array.from({ length: 7 }, (_, index) => 0.18 + 0.32 * Math.abs(Math.sin(index * 0.9 + Date.now() / 400))),
  );
}

export function getRuntimeEvents(): Promise<RuntimeEvent[]> {
  return command("get_runtime_events", undefined, () => [] as RuntimeEvent[]);
}

export function handleDictationAction(action: string): Promise<void> {
  return command("handle_dictation_action", { action }, () => undefined);
}

export function setShortcutTestActive(active: boolean): Promise<void> {
  return command("set_shortcut_test_active", { active }, () => undefined);
}

export function showMainWindow(): Promise<void> {
  return command("show_main_window", undefined, () => undefined);
}

export function listRecentApps(limit: number): Promise<RecentAppUsage[]> {
  return command("list_recent_apps", { limit }, () => {
    const counts = new Map<string, number>();
    for (const session of mockSnapshot.sessions) {
      const name = session.appName ?? "Unknown";
      counts.set(name, (counts.get(name) ?? 0) + 1);
    }
    return Array.from(counts.entries())
      .map(([name, sessionCount]) => ({ name, category: "other", sessionCount }))
      .sort((a, b) => b.sessionCount - a.sessionCount)
      .slice(0, limit);
  });
}

export function searchSessions(filters: HistorySearchFilters): Promise<TranscriptSession[]> {
  return command("search_sessions", { filters }, () => {
    const query = filters.query?.toLowerCase() ?? null;
    return mockSnapshot.sessions
      .filter((session) => {
        if (query && !`${session.cleanedText} ${session.appName ?? ""} ${session.notes}`.toLowerCase().includes(query)) {
          return false;
        }
        if (filters.minWordCount != null && session.wordCount < filters.minWordCount) return false;
        if (filters.maxWordCount != null && session.wordCount > filters.maxWordCount) return false;
        if (filters.fromDate && session.createdAt < filters.fromDate) return false;
        if (filters.toDate && session.createdAt > `${filters.toDate}T23:59:59`) return false;
        return true;
      })
      .slice(0, filters.limit);
  });
}

export function exportSession(id: string, format: ExportFormat): Promise<string> {
  return command("export_session", { id, format }, () => {
    const session = mockSnapshot.sessions.find((candidate) => candidate.id === id);
    const text = session?.cleanedText ?? "";
    if (format === "json") return JSON.stringify(session ?? {}, null, 2);
    if (format === "srt") return `1\n00:00:00,000 --> 00:00:04,000\n${text}\n`;
    if (format === "md") return `# Transcript\n\n${text}\n`;
    return text;
  });
}

export function updateSessionNotes(id: string, notes: string): Promise<AppSnapshot> {
  return command("update_session_notes", { id, notes }, () => {
    mockSnapshot = {
      ...mockSnapshot,
      sessions: mockSnapshot.sessions.map((session) => (session.id === id ? { ...session, notes } : session)),
    };
    return mockSnapshot;
  });
}

export function polishSession(id: string): Promise<PolishResult> {
  return command("polish_session", { id }, () => ({
    snapshot: mockSnapshot,
    polish: { changed: false, style: mockSnapshot.settings.polishStyle },
  }));
}

export function submitFeedback(message: string): Promise<FeedbackResult> {
  return command("submit_feedback", { message }, () => ({
    delivered: false,
    message: "Feedback captured locally in preview mode.",
  }));
}

export function getReleaseArtifacts(): Promise<DownloadArtifact[]> {
  return Promise.resolve(
    releaseArtifacts().map((artifact) => ({
      ...artifact,
      sizeBytes: null,
      sha256: null,
    })),
  );
}
