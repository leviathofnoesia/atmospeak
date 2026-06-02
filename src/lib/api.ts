import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import type {
  AppSettings,
  AppSnapshot,
  DictationResult,
  DictionaryEntry,
  DownloadArtifact,
  InjectionResult,
  MicrophoneInfo,
  ModelInventory,
  ModelStatus,
  RecordingStarted,
  ReleaseArtifact,
  ShortcutStatus,
  Snippet,
  UpdateCheckResult,
} from "../types/dictation";

type InvokeArgs = Record<string, unknown>;

interface WindowWithTauri extends Window {
  __TAURI_INTERNALS__?: unknown;
}

const releaseBaseUrl = "https://github.com/leviathofnoesia/wind-speak/releases/latest/download";

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
};

let mockSnapshot: AppSnapshot = {
  settings: defaultSettings,
  dictionary: [
    {
      id: "mock-dictionary-1",
      phrase: "wind speak",
      replacement: "Wind Speak",
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
      fileName: "Wind-Speak_0.1.6_x64-setup.exe",
      kind: "installer",
      url: `${releaseBaseUrl}/Wind-Speak_0.1.6_x64-setup.exe`,
      recommended: true,
    },
    {
      id: "msi",
      label: "Windows MSI",
      fileName: "Wind-Speak_0.1.6_x64_en-US.msi",
      kind: "msi",
      url: `${releaseBaseUrl}/Wind-Speak_0.1.6_x64_en-US.msi`,
      recommended: false,
    },
    {
      id: "portable",
      label: "Portable zip",
      fileName: "Wind-Speak_0.1.6_x64-portable.zip",
      kind: "portable",
      url: `${releaseBaseUrl}/Wind-Speak_0.1.6_x64-portable.zip`,
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
      "Wind Speak captured this offline prototype transcript and pasted it into the active window.";
    const session = {
      id: crypto.randomUUID(),
      rawText: cleanedText,
      cleanedText,
      audioPath: "mock://recording.wav",
      durationMs: 4200,
      wordCount: cleanedText.split(/\s+/).length,
      injected: mockSnapshot.settings.autoInject,
      createdAt: new Date().toISOString(),
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
      currentVersion: "0.1.6",
      version: null,
      date: null,
      body: null,
      message: "Wind Speak is current in browser preview mode.",
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
      message: "Wind Speak is up to date.",
    };
  }

  return {
    available: update.available,
    currentVersion,
    version: update.version,
    date: update.date ?? null,
    body: update.body ?? null,
    message: update.available
      ? `Wind Speak ${update.version} is ready to install.`
      : "Wind Speak is up to date.",
  };
}

export async function downloadAndInstallUpdate(): Promise<UpdateCheckResult> {
  if (!hasTauriRuntime()) {
    return {
      available: false,
      currentVersion: "0.1.6",
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
      message: "Wind Speak is already up to date.",
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
    message: `Wind Speak ${update.version} installed. Relaunching.`,
  };
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
