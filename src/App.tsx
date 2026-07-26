import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import clsx from "clsx";
import {
  BookOpen,
  History,
  Home,
  Radio,
  Scissors,
  Settings,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { FormEvent } from "react";
import "./App.css";
import "./styles/hub.css";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Aura } from "./components/Aura";
import { AdvancedPanel } from "./components/AdvancedPanel";
import { DictionaryPanel } from "./components/DictionaryPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { HomePanel } from "./components/HomePanel";
import { Onboarding } from "./components/Onboarding";
import { ModelManagement } from "./components/ModelManagement";
import { RecorderOverlay } from "./components/RecorderOverlay";
import { SettingsPanel } from "./components/SettingsPanel";
import { SnippetPanel } from "./components/SnippetPanel";
import type { RecorderPhase } from "./components/RecorderOverlay";
import {
  cancelModelDownload,
  cancelSoundCheck,
  checkForUpdates,
  completeOnboarding as completeOnboardingNative,
  copyText,
  deleteDictionaryEntry,
  deleteModel,
  deleteSession,
  deleteSnippet,
  downloadAndInstallUpdate,
  downloadModel,
  getAppSnapshot,
  getLastStageMetrics,
  getModelInventory,
  getModelStatus,
  getRuntimeEvents,
  getShortcutStatus,
  handleDictationAction,
  hasTauriRuntime,
  injectText,
  listMicrophones,
  openWindowsSoundSettings,
  registerSetupShortcut,
  resetOverlayPosition,
  saveOverlayPosition,
  saveSettings,
  setShortcutTestActive,
  setShortcutsPaused,
  showFloatingControl,
  showMainWindow,
  startSoundCheck,
  finishSoundCheck,
  upsertDictionaryEntry,
  upsertSnippet,
} from "./lib/api";
import type {
  AppNotice,
  AppSettings,
  AppSnapshot,
  DictionaryEntry,
  HubTab,
  MicLevel,
  MicrophoneInfo,
  ModelInventory,
  ModelDownloadProgress,
  ModelStatus,
  NativeDictationEvent,
  RecordingStarted,
  RuntimeEvent,
  ShortcutStatus,
  Snippet,
  SoundCheckResult,
  StageMetrics,
  UpdateCheckResult,
  UpdateStatus,
} from "./types/dictation";
import { ONBOARDING_VERSION } from "./types/dictation";

const tabs: Array<{ id: HubTab; label: string; icon: typeof Radio }> = [
  { id: "home", label: "Home", icon: Home },
  { id: "history", label: "History", icon: History },
  { id: "dictionary", label: "Dictionary", icon: BookOpen },
  { id: "snippets", label: "Snippets", icon: Scissors },
  { id: "settings", label: "Settings", icon: Settings },
];

interface ShortcutTestState {
  active: boolean;
  detected: boolean;
  message: string;
}

interface PasteTestState {
  running: boolean;
  passed: boolean;
  message: string;
}

interface MicCheckState {
  active: boolean;
  passed: boolean;
  level: number;
  message: string;
}

interface SoundCheckState {
  active: boolean;
  result: SoundCheckResult | null;
  message: string;
}

function stringifyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function soundCheckFailureMessage(code: string | null): string {
  switch (code) {
    case "too_short":
      return "Keep holding while you read the complete line.";
    case "too_long":
      return "The check ran too long. Release after the line.";
    case "too_quiet":
      return "Your voice is too quiet. Move closer or raise the Windows input level.";
    case "excessive_noise":
      return "Background noise is masking your voice.";
    case "clipping":
      return "The input is clipping. Lower the Windows input level.";
    case "backend_unavailable":
      return "The resident transcription host is still warming. Wait a moment and retry.";
    case "unintelligible_phrase":
      return "The words did not match the line clearly enough. Try again.";
    default:
      return "The sound check did not pass.";
  }
}

// Browser/native-debug fixture for exercising the shortcut step without
// weakening the production microphone or transcription gates.
const DEV_SHORTCUT_FIXTURE =
  import.meta.env.DEV &&
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).get("fixture") === "shortcut";

function AppShell() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<AppSettings | null>(null);
  const [microphones, setMicrophones] = useState<MicrophoneInfo[]>([]);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [modelInventory, setModelInventory] = useState<ModelInventory | null>(null);
  const [modelDownload, setModelDownload] = useState<ModelDownloadProgress | null>(null);
  const [shortcutStatus, setShortcutStatus] = useState<ShortcutStatus | null>(null);
  const [runtimeEvents, setRuntimeEvents] = useState<RuntimeEvent[]>([]);
  const [lastMetrics, setLastMetrics] = useState<StageMetrics | null>(null);
  const [activeTab, setActiveTab] = useState<HubTab>("home");
  const [notice, setNotice] = useState<AppNotice>({
    tone: "neutral",
    message: "Atmospeak ready.",
  });
  const [recording, setRecording] = useState<RecordingStarted | null>(null);
  const [recorderPhase, setRecorderPhase] = useState<RecorderPhase>("idle");
  const [, setElapsedSeconds] = useState(0);
  const [shortcutTest, setShortcutTest] = useState<ShortcutTestState>({
    active: false,
    detected: false,
    message: "",
  });
  const [micCheck, setMicCheck] = useState<MicCheckState>({
    active: false,
    passed: DEV_SHORTCUT_FIXTURE,
    level: DEV_SHORTCUT_FIXTURE ? 0.72 : 0,
    message: DEV_SHORTCUT_FIXTURE ? "Development shortcut fixture." : "",
  });
  const [, setMicLevel] = useState<MicLevel | null>(null);
  const [soundCheck, setSoundCheck] = useState<SoundCheckState>({
    active: false,
    result: null,
    message: "",
  });
  const [pasteTest, setPasteTest] = useState<PasteTestState>({
    running: false,
    passed: false,
    message: "",
  });
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [dictEntry, setDictEntry] = useState({
    id: null as string | null,
    phrase: "",
    replacement: "",
  });
  const [snippetDraft, setSnippetDraft] = useState({
    id: null as string | null,
    trigger: "",
    body: "",
  });

  const busyRef = useRef(false);
  const recordingRef = useRef<RecordingStarted | null>(null);
  const settingsRef = useRef<AppSettings | null>(null);
  const shortcutTestRef = useRef(shortcutTest);
  const shortcutStatusRef = useRef<ShortcutStatus | null>(null);
  const shortcutTestTimerRef = useRef<number | null>(null);

  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);
  useEffect(() => {
    settingsRef.current = settingsDraft;
  }, [settingsDraft]);
  useEffect(() => {
    shortcutTestRef.current = shortcutTest;
  }, [shortcutTest]);
  useEffect(() => {
    shortcutStatusRef.current = shortcutStatus;
  }, [shortcutStatus]);

  const setBusyState = useCallback((nextBusy: boolean) => {
    busyRef.current = nextBusy;
  }, []);

  const refreshSnapshotOnly = useCallback(async () => {
    const next = await getAppSnapshot();
    setSnapshot(next);
    setSettingsDraft(next.settings);
    return next;
  }, []);

  const refresh = useCallback(async () => {
    const [next, mics, status, inventory, shortcut, events, metrics] = await Promise.all([
      getAppSnapshot(),
      listMicrophones(),
      getModelStatus(),
      getModelInventory(),
      getShortcutStatus(),
      getRuntimeEvents(),
      getLastStageMetrics(),
    ]);
    setSnapshot(next);
    const preferredMicrophone =
      next.settings.microphoneName ??
      mics.find((microphone) => microphone.isDefault)?.name ??
      mics[0]?.name ??
      null;
    setSettingsDraft({ ...next.settings, microphoneName: preferredMicrophone });
    setMicrophones(mics);
    setModelStatus(status);
    setModelInventory(inventory);
    setShortcutStatus(shortcut);
    setRuntimeEvents(events);
    setLastMetrics(metrics);
    return next;
  }, []);

  useEffect(() => {
    void refresh().catch((error: unknown) => {
      setNotice({ tone: "error", message: stringifyError(error) });
    });
  }, [refresh]);

  useEffect(() => {
    if (recording === null) {
      setElapsedSeconds(0);
      return undefined;
    }
    const startedAt = new Date(recording.startedAt).getTime();
    const interval = window.setInterval(() => {
      setElapsedSeconds(Math.max(0, (Date.now() - startedAt) / 1000));
    }, 1000);
    return () => window.clearInterval(interval);
  }, [recording]);

  const applyNativeDictation = useCallback(
    (payload: NativeDictationEvent) => {
      setRecording(payload.recording);
      setRecorderPhase(payload.phase);
      setBusyState(payload.phase === "processing" || payload.phase === "listening");
      setNotice({
        tone:
          payload.phase === "error"
            ? "error"
            : payload.phase === "pasted"
              ? "success"
              : "neutral",
        message: payload.message,
      });
      if (payload.metrics) {
        setLastMetrics(payload.metrics);
      }
      if (payload.result !== null) {
        void refreshSnapshotOnly();
        setActiveTab("history");
      }
    },
    [refreshSnapshotOnly, setBusyState],
  );

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    void (async () => {
      const pairs: Array<[string, (payload: unknown) => void]> = [
        [
          "wind-speak://shortcut",
          (payload) => {
            const action = String(payload);
            if (!shortcutTestRef.current.active) {
              // Dictation is owned by Rust DictationEngine after Phase A3.
              return;
            }
            if (shortcutStatusRef.current?.paused) {
              setShortcutTest({
                active: false,
                detected: false,
                message: "Shortcuts are paused. Resume shortcuts and test again.",
              });
              return;
            }
            if (action === "pressed") {
              setShortcutTest((current) => ({
                ...current,
                message: "Chord detected. Release the keys to finish the test.",
              }));
              return;
            }
            if (action === "toggle" || action === "released") {
              if (shortcutTestTimerRef.current !== null) {
                window.clearTimeout(shortcutTestTimerRef.current);
                shortcutTestTimerRef.current = null;
              }
              const label =
                shortcutStatusRef.current?.hotkey ||
                settingsRef.current?.hotkey ||
                "shortcut";
              void setShortcutTestActive(false);
              setShortcutTest({
                active: false,
                detected: true,
                message: `${label} detected by the desktop runtime.`,
              });
              setNotice({ tone: "success", message: `${label} detected.` });
            }
          },
        ],
        [
          "wind-speak://shortcut-status",
          (payload) => {
            const next = payload as ShortcutStatus;
            shortcutStatusRef.current = next;
            setShortcutStatus(next);
          },
        ],
        [
          "wind-speak://native-dictation",
          (payload) => applyNativeDictation(payload as NativeDictationEvent),
        ],
        [
          "atmospeak://native-dictation",
          (payload) => applyNativeDictation(payload as NativeDictationEvent),
        ],
        [
          "wind-speak://runtime-event",
          (payload) => {
            const event = payload as RuntimeEvent;
            setRuntimeEvents((current) => [event, ...current].slice(0, 100));
          },
        ],
        [
          "wind-speak://stage-metrics",
          (payload) => setLastMetrics(payload as StageMetrics),
        ],
        [
          "atmospeak://model-download",
          (payload) => setModelDownload(payload as ModelDownloadProgress),
        ],
        [
          "atmospeak://mic-level",
          (payload) => {
            const level = payload as MicLevel;
            setMicLevel(level);
            setMicCheck((current) => {
              if (!current.active) return current;
              const healthy = level.rmsDbfs >= -48 && level.peakDbfs >= -30;
              return {
                ...current,
                passed: current.passed || healthy,
                level: Math.max(0, Math.min(1, (level.peakDbfs + 60) / 60)),
                message: healthy
                  ? `Healthy signal · ${level.rmsDbfs.toFixed(1)} dBFS RMS`
                  : `Too quiet · ${level.rmsDbfs.toFixed(1)} dBFS RMS`,
              };
            });
          },
        ],
        [
          "atmospeak://sound-check",
          (payload) => {
            const result = payload as SoundCheckResult;
            setSoundCheck({
              active: false,
              result,
              message: result.passed
                ? `Heard clearly in ${result.asrMs}ms on the resident host.`
                : soundCheckFailureMessage(result.failureCode),
            });
          },
        ],
        [
          "wind-speak://overlay-visibility",
          (payload) =>
            setNotice({ tone: "neutral", message: String(payload) }),
        ],
      ];

      for (const [event, handler] of pairs) {
        const unlisten = await listen(event, (e) => handler(e.payload));
        if (cancelled) {
          unlisten();
        } else {
          unlisteners.push(unlisten);
        }
      }
    })();

    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
      if (shortcutTestTimerRef.current !== null) {
        window.clearTimeout(shortcutTestTimerRef.current);
        shortcutTestTimerRef.current = null;
      }
    };
  }, [applyNativeDictation]);

  const startMicCheck = useCallback(async () => {
    if (busyRef.current || recordingRef.current !== null) {
      setMicCheck((current) => ({
        ...current,
        message: "Stop the current recording before checking the microphone.",
      }));
      return;
    }
    setBusyState(true);
    try {
      const deviceName = settingsRef.current?.microphoneName;
      if (!deviceName) throw new Error("Choose a microphone before checking its signal.");
      await startSoundCheck(deviceName);
      setMicCheck({
        active: true,
        passed: false,
        level: 0,
        message: `Listening through ${deviceName}...`,
      });
      setNotice({ tone: "neutral", message: "Microphone check is listening." });
    } catch (error: unknown) {
      setMicCheck({
        active: false,
        passed: false,
        level: 0,
        message: stringifyError(error),
      });
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusyState(false);
    }
  }, [setBusyState]);

  const stopMicCheck = useCallback(async () => {
    await cancelSoundCheck().catch(() => false);
    setMicCheck((current) => ({
      ...current,
      active: false,
      message: current.passed
        ? "Healthy microphone signal confirmed."
        : "No healthy signal was confirmed.",
    }));
  }, []);

  const startPhraseCheck = useCallback(async () => {
    const currentSettings = settingsRef.current;
    const deviceName = currentSettings?.microphoneName;
    if (!deviceName) {
      setSoundCheck({ active: false, result: null, message: "Choose a microphone first." });
      return;
    }
    try {
      // Persist the selected device and model before capture so the resident
      // host used by the mandatory check is the one the user actually chose.
      if (currentSettings) {
        const prepared = await saveSettings({
          ...currentSettings,
          onboardingComplete: false,
          onboardingVersion: "",
          audioCalibration: null,
        });
        setSnapshot(prepared);
        setSettingsDraft(prepared.settings);
      }
      await startSoundCheck(deviceName);
      setSoundCheck({ active: true, result: null, message: "Listening..." });
    } catch (error: unknown) {
      setSoundCheck({ active: false, result: null, message: stringifyError(error) });
    }
  }, []);

  const finishPhraseCheck = useCallback(async () => {
    setSoundCheck((current) => ({ ...current, active: false, message: "Transcribing locally..." }));
    try {
      const result = await finishSoundCheck("The porcelain moon hums over the studio.");
      setSoundCheck({
        active: false,
        result,
        message: result.passed
          ? `Heard clearly in ${result.asrMs}ms on the resident host.`
          : soundCheckFailureMessage(result.failureCode),
      });
      if (result.passed) {
        const next = await refreshSnapshotOnly();
        setSettingsDraft(next.settings);
      }
    } catch (error: unknown) {
      setSoundCheck({ active: false, result: null, message: stringifyError(error) });
    }
  }, [refreshSnapshotOnly]);

  const onSaveSettings = useCallback(async () => {
    if (!settingsDraft) return;
    setBusyState(true);
    try {
      const next = await saveSettings(settingsDraft);
      setSnapshot(next);
      setSettingsDraft(next.settings);
      const shortcut = await getShortcutStatus();
      setShortcutStatus(shortcut);
      setNotice({ tone: "success", message: "Settings saved." });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusyState(false);
    }
  }, [setBusyState, settingsDraft]);

  const onDownloadModel = useCallback(async (modelId: string) => {
    setModelDownload({
      modelId,
      status: "starting",
      bytesDownloaded: 0,
      totalBytes: null,
      percent: 0,
      message: "Starting model download.",
    });
    try {
      const inventory = await downloadModel(modelId);
      setModelInventory(inventory);
      setSettingsDraft((current) =>
        current ? { ...current, activeModelId: modelId } : current,
      );
      setModelStatus(await getModelStatus());
      setNotice({
        tone: "success",
        message: `${modelId} is installed. Save settings or finish setup to use it.`,
      });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
      throw error;
    }
  }, []);

  const onDeleteModel = useCallback(async (modelId: string) => {
    try {
      const inventory = await deleteModel(modelId);
      setModelInventory(inventory);
      setSettingsDraft((current) =>
        current?.activeModelId === modelId
          ? { ...current, activeModelId: "base.en" }
          : current,
      );
      setModelStatus(await getModelStatus());
      setNotice({ tone: "success", message: `${modelId} was removed.` });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    }
  }, []);

  const needsOnboarding = useMemo(() => {
    if (!snapshot) return false;
    return (
      !snapshot.settings.onboardingComplete ||
      snapshot.settings.onboardingVersion !== ONBOARDING_VERSION ||
      snapshot.settings.audioCalibration?.asrBackend !== "host"
    );
  }, [snapshot]);

  const completeOnboarding = useCallback(async () => {
    if (!settingsDraft) return;
    if (!soundCheck.result?.passed) {
      throw new Error("Complete the sound check before entering Atmospeak.");
    }
    const next = await completeOnboardingNative(settingsDraft);
    setSnapshot(next);
    setSettingsDraft(next.settings);
    setNotice({ tone: "success", message: "Onboarding complete. Atmospeak is armed." });
  }, [settingsDraft, soundCheck.result]);

  const recentSession = snapshot?.sessions[0] ?? null;

  if (!snapshot || !settingsDraft) {
    return (
      <div className="boot-shell">
        <Aura size={64} active />
        <p>Loading Atmospeak…</p>
      </div>
    );
  }

  if (needsOnboarding) {
    return (
      <Onboarding
        settings={settingsDraft}
        setSettings={setSettingsDraft}
        microphones={microphones}
        modelStatus={modelStatus}
        modelInventory={modelInventory}
        modelDownload={modelDownload}
        shortcutStatus={shortcutStatus}
        shortcutTest={shortcutTest}
        micCheck={micCheck}
        soundCheck={soundCheck}
        onStartMicCheck={startMicCheck}
        onStopMicCheck={stopMicCheck}
        onStartSoundCheck={startPhraseCheck}
        onFinishSoundCheck={finishPhraseCheck}
        onOpenWindowsSoundSettings={openWindowsSoundSettings}
        onTestShortcut={() => {
          if (shortcutTestTimerRef.current !== null) {
            window.clearTimeout(shortcutTestTimerRef.current);
            shortcutTestTimerRef.current = null;
          }
          setShortcutTest({
            active: true,
            detected: false,
            message: "Arming the Windows shortcut hook...",
          });
          void registerSetupShortcut(settingsDraft.hotkey)
            .then((status) => {
              shortcutStatusRef.current = status;
              setShortcutStatus(status);
              setShortcutTest({
                active: status.registered,
                detected: false,
                message: status.registered ? "Press your dictation shortcut…" : status.message,
              });
              if (shortcutTestTimerRef.current !== null) {
                window.clearTimeout(shortcutTestTimerRef.current);
              }
              if (status.registered) {
                shortcutTestTimerRef.current = window.setTimeout(() => {
                  shortcutTestTimerRef.current = null;
                  void setShortcutTestActive(false);
                  setShortcutTest({
                    active: false,
                    detected: false,
                    message: "Shortcut was not detected. Choose another chord and retry.",
                  });
                }, 15_000);
              }
            })
            .catch((error: unknown) => {
              void setShortcutTestActive(false);
              setShortcutTest({
                active: false,
                detected: false,
                message: stringifyError(error),
              });
            });
        }}
        onCancelShortcutTest={() => {
          if (shortcutTestTimerRef.current !== null) {
            window.clearTimeout(shortcutTestTimerRef.current);
            shortcutTestTimerRef.current = null;
          }
          void setShortcutTestActive(false);
          setShortcutTest((current) => ({
            ...current,
            active: false,
            message: current.detected
              ? current.message
              : "Shortcut test cancelled. Test the selected chord to continue.",
          }));
        }}
        onShortcutChange={(hotkey) => {
          if (shortcutTestTimerRef.current !== null) {
            window.clearTimeout(shortcutTestTimerRef.current);
            shortcutTestTimerRef.current = null;
          }
          void setShortcutTestActive(false);
          setShortcutTest({
            active: false,
            detected: false,
            message: "Selection changed. Test this chord to continue.",
          });
          setSettingsDraft({ ...settingsDraft, hotkey });
        }}
        pasteTest={pasteTest}
        onPasteTest={async () => {
          setPasteTest({ running: true, passed: false, message: "Pasting sample…" });
          try {
            const result = await injectText("The porcelain moon hums over the studio.");
            setPasteTest({
              running: false,
              passed: result.injected || result.message.includes("clipboard"),
              message: result.message,
            });
          } catch (error: unknown) {
            setPasteTest({
              running: false,
              passed: false,
              message: stringifyError(error),
            });
          }
        }}
        onSelectModel={(modelId) =>
          setSettingsDraft((current) =>
            current ? { ...current, activeModelId: modelId } : current,
          )
        }
        onDownloadModel={onDownloadModel}
        onCancelModelDownload={async () => {
          await cancelModelDownload();
        }}
        onComplete={completeOnboarding}
      />
    );
  }

  return (
    <div className="hub-shell">
      <aside className="hub__nav">
        <div className="hub__brand">
          <span className="brand-aura">
            <Aura size={30} active={recorderPhase === "listening"} />
          </span>
          <span className="wm">
            <strong>Atmospeak</strong>
            <span>Local · {settingsDraft.accent}</span>
          </span>
        </div>
        <nav aria-label="Atmospeak sections">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            const count =
              tab.id === "history"
                ? snapshot.sessions.length
                : tab.id === "dictionary"
                  ? snapshot.dictionary.length
                  : tab.id === "snippets"
                    ? snapshot.snippets.length
                    : null;
            return (
              <button
                key={tab.id}
                type="button"
                aria-label={tab.label}
                className={clsx("hub__navitem", activeTab === tab.id && "active")}
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={18} />
                <span className="lab">{tab.label}</span>
                {count !== null ? <span className="ct">{count}</span> : null}
              </button>
            );
          })}
        </nav>
        <div className="marquee-foot">
          + ON DEVICE · NO CLOUD · WHISPER · {modelInventory?.activeModelId ?? "BASE.EN"} +
        </div>
      </aside>
      <main className="hub__main">
        <button
          type="button"
          className="hub__close"
          aria-label="Close hub"
          onClick={() => {
            if (hasTauriRuntime()) void getCurrentWindow().close();
          }}
        >
          <X size={16} />
        </button>
        {notice.tone !== "neutral" ? (
          <div className={`notice-rail notice-rail--${notice.tone}`} role="status">
            <span className={`notice-rail__tone notice-rail__tone--${notice.tone}`} />
            <p>{notice.message}</p>
          </div>
        ) : null}
        {activeTab === "home" ? (
          <HomePanel
            snapshot={snapshot}
            recentSession={recentSession}
            onCopyRecent={async (session) => {
              const message = await copyText(session.cleanedText);
              setNotice({ tone: "success", message });
            }}
          />
        ) : null}
        {activeTab === "history" ? (
          <HistoryPanel
            sessions={snapshot.sessions}
            onCopy={async (session) => {
              const message = await copyText(session.cleanedText);
              setNotice({ tone: "success", message });
            }}
            onInject={async (session) => {
              const result = await injectText(session.cleanedText);
              setNotice({
                tone: result.injected ? "success" : "error",
                message: result.message,
              });
            }}
            onDelete={async (session) => {
              const next = await deleteSession(session.id);
              setSnapshot(next);
              setNotice({ tone: "success", message: "Transcript deleted." });
            }}
          />
        ) : null}
        {activeTab === "dictionary" ? (
          <DictionaryPanel
            entries={snapshot.dictionary}
            draft={dictEntry}
            setDraft={setDictEntry}
            onSubmit={async (event: FormEvent) => {
              event.preventDefault();
              const next = await upsertDictionaryEntry({
                id: dictEntry.id ?? "",
                phrase: dictEntry.phrase,
                replacement: dictEntry.replacement,
                enabled: true,
                createdAt: new Date().toISOString(),
              });
              setSnapshot(next);
              setDictEntry({ id: null, phrase: "", replacement: "" });
            }}
            onToggle={async (entry: DictionaryEntry) => {
              const next = await upsertDictionaryEntry({
                ...entry,
                enabled: !entry.enabled,
              });
              setSnapshot(next);
            }}
            onDelete={async (entry: DictionaryEntry) => {
              const next = await deleteDictionaryEntry(entry.id);
              setSnapshot(next);
            }}
          />
        ) : null}
        {activeTab === "snippets" ? (
          <SnippetPanel
            snippets={snapshot.snippets}
            draft={snippetDraft}
            setDraft={setSnippetDraft}
            onSubmit={async (event: FormEvent) => {
              event.preventDefault();
              const next = await upsertSnippet({
                id: snippetDraft.id ?? "",
                trigger: snippetDraft.trigger,
                body: snippetDraft.body,
                enabled: true,
                createdAt: new Date().toISOString(),
              });
              setSnapshot(next);
              setSnippetDraft({ id: null, trigger: "", body: "" });
            }}
            onToggle={async (snippet: Snippet) => {
              const next = await upsertSnippet({
                ...snippet,
                enabled: !snippet.enabled,
              });
              setSnapshot(next);
            }}
            onDelete={async (snippet: Snippet) => {
              const next = await deleteSnippet(snippet.id);
              setSnapshot(next);
            }}
          />
        ) : null}
        {activeTab === "settings" ? (
          <SettingsPanel
            settings={settingsDraft}
            setSettings={setSettingsDraft}
            microphones={microphones}
            shortcutStatus={shortcutStatus}
            shortcutTest={shortcutTest}
            onTestShortcut={() => {
              if (shortcutStatus?.paused) {
                setShortcutTest({
                  active: false,
                  detected: false,
                  message: "Shortcuts are paused. Resume shortcuts and test again.",
                });
                setNotice({
                  tone: "warning",
                  message: "Shortcuts are paused. Resume shortcuts and test again.",
                });
                return;
              }
              void setShortcutTestActive(true);
              setShortcutTest({
                active: true,
                detected: false,
                message: "Press your dictation shortcut…",
              });
            }}
            onToggleShortcutsPaused={async () => {
              const next = await setShortcutsPaused(!(shortcutStatus?.paused ?? false));
              setShortcutStatus(next);
            }}
            onShowFloatingControl={async () => {
              await showFloatingControl();
              setNotice({ tone: "success", message: "Floating control shown." });
            }}
            onResetDockPosition={async () => {
              await resetOverlayPosition();
              setNotice({ tone: "success", message: "Dock position reset." });
            }}
            onRerunOnboarding={async () => {
              const next = await saveSettings({
                ...settingsDraft,
                onboardingComplete: false,
                onboardingVersion: "",
                audioCalibration: null,
              });
              setSnapshot(next);
              setSettingsDraft(next.settings);
            }}
            onSave={onSaveSettings}
            updateStatus={updateStatus}
            updateResult={updateResult}
            onCheckUpdates={async () => {
              setUpdateStatus("checking");
              try {
                const result = await checkForUpdates();
                setUpdateResult(result);
                setUpdateStatus(result.available ? "available" : "current");
              } catch (error: unknown) {
                setUpdateStatus("error");
                setNotice({ tone: "error", message: stringifyError(error) });
              }
            }}
            onInstallUpdate={async () => {
              setUpdateStatus("downloading");
              try {
                const result = await downloadAndInstallUpdate();
                setUpdateResult(result);
                setUpdateStatus("readyToRelaunch");
              } catch (error: unknown) {
                setUpdateStatus("error");
                setNotice({ tone: "error", message: stringifyError(error) });
              }
            }}
            advanced={
              <AdvancedPanel
                settings={settingsDraft}
                setSettings={setSettingsDraft}
                modelStatus={modelStatus}
                lastMetrics={lastMetrics}
                runtimeEvents={runtimeEvents}
                onRunDiagnosticSoundCheck={async () => {
                  const next = await saveSettings({
                    ...settingsDraft,
                    onboardingComplete: false,
                    onboardingVersion: "",
                    audioCalibration: null,
                  });
                  setSnapshot(next);
                  setSettingsDraft(next.settings);
                }}
                onSave={onSaveSettings}
              />
            }
            modelManagement={
              <ModelManagement
                settings={settingsDraft}
                inventory={modelInventory}
                download={modelDownload}
                onSelect={(modelId) =>
                  setSettingsDraft((current) =>
                    current ? { ...current, activeModelId: modelId } : current,
                  )
                }
                onDownload={onDownloadModel}
                onCancelDownload={async () => {
                  await cancelModelDownload();
                }}
                onDelete={onDeleteModel}
              />
            }
          />
        ) : null}
      </main>
    </div>
  );
}

function OverlayShell() {
  const [phase, setPhase] = useState<RecorderPhase>("idle");
  const [message, setMessage] = useState("Ready");
  const [level, setLevel] = useState(0);
  const [dragDiagnostic, setDragDiagnostic] = useState("");
  const suppressPositionSaveRef = useRef(false);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [recording, setRecording] = useState<RecordingStarted | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [hostApp, setHostApp] = useState("your cursor");

  // The rest tip names the real chord and gesture, so it has to follow settings.
  // Model status greys the tip out when the speech runtime is missing, so the dock
  // never invites a dictation that cannot run.
  useEffect(() => {
    void getAppSnapshot()
      .then((snapshot) => setSettings(snapshot.settings))
      .catch(() => {
        /* overlay still renders without it */
      });
    void getModelStatus()
      .then(setModelStatus)
      .catch(() => {
        /* treated as ready; the engine reports the real error on use */
      });
  }, []);

  useEffect(() => {
    if (!hasTauriRuntime()) return undefined;
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    void (async () => {
      const unlisten = await listen<NativeDictationEvent>(
        "wind-speak://native-dictation",
        (event) => {
          setPhase(event.payload.phase);
          setMessage(event.payload.message);
          setRecording(event.payload.recording);
          // "Set down in Notepad" — named from where the text actually landed.
          const target = event.payload.result?.injection?.targetProcessName;
          if (target) setHostApp(target);
        },
      );
      if (cancelled) unlisten();
      else unlisteners.push(unlisten);

      // Appearance and hotkey changes are made in the hub's window, not this one.
      const unlistenSettings = await listen<AppSettings>(
        "wind-speak://settings-changed",
        (event) => setSettings(event.payload),
      );
      if (cancelled) unlistenSettings();
      else unlisteners.push(unlistenSettings);

      const unlistenLevel = await listen<MicLevel>(
        "atmospeak://mic-level",
        (event) => {
          const normalizedPeak = (event.payload.peakDbfs + 60) / 60;
          setLevel(Math.max(0, Math.min(1, normalizedPeak)));
        },
      );
      if (cancelled) unlistenLevel();
      else unlisteners.push(unlistenLevel);

      let resetTimer: number | undefined;
      const unlistenReset = await listen(
        "atmospeak://overlay-position-resetting",
        () => {
          suppressPositionSaveRef.current = true;
          window.clearTimeout(resetTimer);
          resetTimer = window.setTimeout(() => {
            suppressPositionSaveRef.current = false;
          }, 1_000);
        },
      );
      if (cancelled) unlistenReset();
      else
        unlisteners.push(() => {
          window.clearTimeout(resetTimer);
          unlistenReset();
        });

      // Remember where the companion was dropped. onMoved fires throughout the OS
      // drag, so settle briefly before writing.
      let moveTimer: number | undefined;
      const unlistenMoved = await getCurrentWindow().onMoved(({ payload }) => {
        if (suppressPositionSaveRef.current) return;
        window.clearTimeout(moveTimer);
        moveTimer = window.setTimeout(() => {
          void saveOverlayPosition(payload.x, payload.y).catch(() => undefined);
        }, 400);
      });
      if (cancelled) unlistenMoved();
      else
        unlisteners.push(() => {
          window.clearTimeout(moveTimer);
          unlistenMoved();
        });
    })();
    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);

  useEffect(() => {
    if (phase !== "listening") setLevel(0);
  }, [phase]);

  useEffect(() => {
    if (!recording) {
      setElapsedSeconds(0);
      return undefined;
    }
    const startedAt = new Date(recording.startedAt).getTime();
    const interval = window.setInterval(() => {
      setElapsedSeconds(Math.max(0, (Date.now() - startedAt) / 1000));
    }, 500);
    return () => window.clearInterval(interval);
  }, [recording]);

  return (
    <RecorderOverlay
      recording={recording}
      elapsedSeconds={elapsedSeconds}
      busy={phase === "processing"}
      phase={phase}
      modelStatus={modelStatus}
      notice={dragDiagnostic || message}
      inputLevel={level}
      inputBands={[]}
      bubbleSize="medium"
      bubbleOpacity={1}
      hostApp={hostApp}
      hotkeyLabel={settings?.hotkey ?? "your shortcut"}
      mode={settings?.mode ?? "pushToTalk"}
      accent={settings?.accent ?? "dusk"}
      dockShape={settings?.dockShape ?? "orb"}
      waveStyle={settings?.waveStyle ?? "ribbon"}
      theme={settings?.dockTheme ?? "dark"}
      motion={settings?.motion ?? "lively"}
      onToggle={() => {
        void handleDictationAction("toggle");
      }}
      onCancel={() => {
        void handleDictationAction("cancel");
      }}
      onMoveStart={() => {
        // The dock is 66px, so the cursor leaves it almost immediately and the
        // webview stops seeing pointer moves. The OS move loop is what handles
        // that, but it ignores an inactive window — and the companion is
        // deliberately `focus: false` so it never steals focus while you dictate.
        // Activating it only for the drag is safe: the paste target is captured
        // when listening starts and Atmospeak's own windows are never recorded
        // as a target.
        if (!hasTauriRuntime()) return;
        const overlay = getCurrentWindow();
        setDragDiagnostic("");
        void overlay
          .setFocus()
          .then(() => overlay.startDragging())
          .catch((error: unknown) => {
            console.error("Atmospeak dock drag failed", {
              window: overlay.label,
              error,
            });
            if (import.meta.env.DEV) {
              setDragDiagnostic("Dock drag failed — see the developer console.");
            }
          });
      }}
      onOpenHub={() => {
        void showMainWindow();
      }}
    />
  );
}

/// Marks the document as the transparent overlay window. Exported so `main.tsx`
/// can apply it before first paint; also called on the window-label fallback path.
export function markOverlayDocument() {
  document.documentElement.classList.add("is-overlay-window");
  document.body.classList.add("is-overlay-window");
}

export default function App() {
  const [isOverlay, setIsOverlay] = useState(false);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (params.get("view") === "overlay") {
      markOverlayDocument();
      setIsOverlay(true);
      return;
    }
    if (hasTauriRuntime()) {
      try {
        const label = getCurrentWindow().label;
        if (label === "overlay") {
          markOverlayDocument();
          setIsOverlay(true);
        }
      } catch {
        /* browser mock */
      }
    }
  }, []);

  return (
    <ErrorBoundary>
      {isOverlay ? <OverlayShell /> : <AppShell />}
    </ErrorBoundary>
  );
}
