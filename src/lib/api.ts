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
  InjectionResult,
  MicrophoneInfo,
  ModelInventory,
  ModelStatus,
  RecordingStarted,
  ReleaseArtifact,
  RuntimeEvent,
  ShortcutStatus,
  Snippet,
  StageMetrics,
  UpdateCheckResult,
} from "../types/dictation";
import { defaultSettings } from "../types/dictation";

type InvokeArgs = Record<string, unknown>;

interface WindowWithTauri extends Window {
  __TAURI_INTERNALS__?: unknown;
}

const releaseBaseUrl = "https://github.com/leviathofnoesia/wind-speak/releases/latest/download";
const releaseVersion = "0.3.0";
const mockInstalledModels = new Set(["base.en"]);

let mockSnapshot: AppSnapshot = {
  settings: defaultSettings(),
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

export function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && Boolean((window as WindowWithTauri).__TAURI_INTERNALS__);
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
            restoredTarget: false,
            targetProcessName: null,
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
    restoredTarget: false,
    targetProcessName: null,
    message: "Mock transcript copied to the focused application.",
  }));
}

export async function copyText(text: string): Promise<string> {
  if (text.trim().length === 0) {
    throw new Error("cannot copy an empty transcript");
  }
  if (hasTauriRuntime()) {
    await writeText(text);
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
  return command("get_model_inventory", undefined, () => {
    const models: ModelInventory["models"] = [
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
        installed: mockInstalledModels.has("tiny.en"),
        bundled: false,
        path: mockInstalledModels.has("tiny.en") ? "mock://models/ggml-tiny.en.bin" : null,
        sizeMb: 75,
      },
      {
        id: "small.en",
        label: "Small English",
        installed: mockInstalledModels.has("small.en"),
        bundled: false,
        path: mockInstalledModels.has("small.en") ? "mock://models/ggml-small.en.bin" : null,
        sizeMb: 466,
      },
      {
        id: "medium.en",
        label: "Medium English",
        installed: mockInstalledModels.has("medium.en"),
        bundled: false,
        path: mockInstalledModels.has("medium.en") ? "mock://models/ggml-medium.en.bin" : null,
        sizeMb: 1463,
      },
      {
        id: "distil-large-v3",
        label: "Distil Large v3",
        installed: mockInstalledModels.has("distil-large-v3"),
        bundled: false,
        path: mockInstalledModels.has("distil-large-v3")
          ? "mock://models/ggml-distil-large-v3.bin"
          : null,
        sizeMb: 1450,
      },
    ];
    return {
      activeModelId: mockSnapshot.settings.advancedRuntimeEnabled
        ? "advanced-override"
        : mockInstalledModels.has(mockSnapshot.settings.activeModelId)
          ? mockSnapshot.settings.activeModelId
          : "base.en",
      models,
    };
  });
}

export function downloadModel(modelId: string): Promise<ModelInventory> {
  return command("download_model", { modelId }, async () => {
    mockInstalledModels.add(modelId);
    return getModelInventory();
  });
}

export function cancelModelDownload(): Promise<boolean> {
  return command("cancel_model_download", undefined, () => false);
}

export function deleteModel(modelId: string): Promise<ModelInventory> {
  return command("delete_model", { modelId }, async () => {
    mockInstalledModels.delete(modelId);
    return getModelInventory();
  });
}

export async function checkForUpdates(): Promise<UpdateCheckResult> {
  if (!hasTauriRuntime()) {
    return {
      available: false,
      currentVersion: releaseVersion,
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
      currentVersion: releaseVersion,
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

export function getRuntimeEvents(): Promise<RuntimeEvent[]> {
  return command("get_runtime_events", undefined, () => [] as RuntimeEvent[]);
}

export function getLastStageMetrics(): Promise<StageMetrics | null> {
  return command("get_last_stage_metrics", undefined, () => null);
}

export function handleDictationAction(action: string): Promise<string> {
  return command("handle_dictation_action", { action }, () => "accepted");
}

export function setShortcutTestActive(active: boolean): Promise<void> {
  return command("set_shortcut_test_active", { active }, () => undefined);
}

export function showMainWindow(): Promise<void> {
  return command("show_main_window", undefined, () => undefined);
}

export function saveOverlayPosition(x: number, y: number): Promise<void> {
  return command("save_overlay_position", { x, y }, () => undefined);
}

export function micCheckStart(): Promise<void> {
  return command("mic_check_start", undefined, () => undefined);
}

export function micCheckStop(): Promise<void> {
  return command("mic_check_stop", undefined, () => undefined);
}

export function listReleaseArtifacts(): ReleaseArtifact[] {
  return [
    {
      id: "nsis",
      label: "Windows installer (NSIS)",
      fileName: `atmospeak_${releaseVersion}_x64-setup.exe`,
      kind: "installer",
      url: `${releaseBaseUrl}/atmospeak_${releaseVersion}_x64-setup.exe`,
      recommended: true,
    },
    {
      id: "msi",
      label: "Windows installer (MSI)",
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
  ];
}

export type { DownloadArtifact };
