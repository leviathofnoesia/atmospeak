import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import clsx from "clsx";
import {
  BookOpen,
  Cpu,
  History,
  Radio,
  Scissors,
  Settings,
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
import { RecorderOverlay } from "./components/RecorderOverlay";
import { SettingsPanel } from "./components/SettingsPanel";
import { SnippetPanel } from "./components/SnippetPanel";
import type { RecorderPhase } from "./components/RecorderOverlay";
import { StatusLed } from "./components/StatusLed";
import {
  cancelRecording,
  checkForUpdates,
  copyText,
  deleteDictionaryEntry,
  deleteSnippet,
  downloadAndInstallUpdate,
  getAppSnapshot,
  getLastStageMetrics,
  getModelInventory,
  getModelStatus,
  getRecordingLevel,
  getRuntimeEvents,
  getShortcutStatus,
  handleDictationAction,
  hasTauriRuntime,
  injectText,
  listMicrophones,
  micCheckStart,
  micCheckStop,
  saveOverlayPosition,
  saveSettings,
  setShortcutTestActive,
  setShortcutsPaused,
  showFloatingControl,
  showMainWindow,
  startRecording,
  stopRecording,
  upsertDictionaryEntry,
  upsertSnippet,
} from "./lib/api";
import type {
  AppNotice,
  AppSettings,
  AppSnapshot,
  DictionaryEntry,
  HubTab,
  MicrophoneInfo,
  ModelInventory,
  ModelStatus,
  NativeDictationEvent,
  RecordingStarted,
  RuntimeEvent,
  ShortcutStatus,
  Snippet,
  StageMetrics,
  UpdateCheckResult,
  UpdateStatus,
} from "./types/dictation";
import { ONBOARDING_VERSION } from "./types/dictation";

const recordingLevelPollMs = 250;
const recordingLevelCommitMs = 400;
const recordingLevelDelta = 0.03;

const tabs: Array<{ id: HubTab; label: string; icon: typeof Radio }> = [
  { id: "home", label: "Home", icon: Radio },
  { id: "history", label: "History", icon: History },
  { id: "dictionary", label: "Dictionary", icon: BookOpen },
  { id: "snippets", label: "Snippets", icon: Scissors },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "advanced", label: "Advanced", icon: Cpu },
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

function stringifyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function AppShell() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<AppSettings | null>(null);
  const [microphones, setMicrophones] = useState<MicrophoneInfo[]>([]);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [modelInventory, setModelInventory] = useState<ModelInventory | null>(null);
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
  const [busy, setBusy] = useState(false);
  const [, setElapsedSeconds] = useState(0);
  const [, setRecordingLevel] = useState(0);
  const [shortcutTest, setShortcutTest] = useState<ShortcutTestState>({
    active: false,
    detected: false,
    message: "",
  });
  const [micCheck, setMicCheck] = useState<MicCheckState>({
    active: false,
    passed: false,
    level: 0,
    message: "",
  });
  const [pasteTest, setPasteTest] = useState<PasteTestState>({
    running: false,
    passed: false,
    message: "",
  });
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [dictEntry, setDictEntry] = useState({ phrase: "", replacement: "" });
  const [snippetDraft, setSnippetDraft] = useState({ trigger: "", body: "" });

  const busyRef = useRef(false);
  const recordingRef = useRef<RecordingStarted | null>(null);
  const settingsRef = useRef<AppSettings | null>(null);
  const shortcutTestRef = useRef(shortcutTest);
  const micCheckLevelRef = useRef(0);
  const micCheckPassedRef = useRef(false);
  const lastMicCheckLevelCommitRef = useRef(0);

  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);
  useEffect(() => {
    settingsRef.current = settingsDraft;
  }, [settingsDraft]);
  useEffect(() => {
    shortcutTestRef.current = shortcutTest;
  }, [shortcutTest]);

  const setBusyState = useCallback((nextBusy: boolean) => {
    busyRef.current = nextBusy;
    setBusy(nextBusy);
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
    setSettingsDraft(next.settings);
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

  useEffect(() => {
    if (recorderPhase !== "listening" && !micCheck.active) {
      setRecordingLevel(0);
      return undefined;
    }
    let cancelled = false;
    const poll = () => {
      void getRecordingLevel()
        .then((level) => {
          if (!cancelled) setRecordingLevel(Math.max(0, Math.min(1, level)));
        })
        .catch(() => undefined);
    };
    poll();
    const interval = window.setInterval(poll, recordingLevelPollMs);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [micCheck.active, recorderPhase]);

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
            if (shortcutStatus?.paused) {
              setShortcutTest({
                active: false,
                detected: false,
                message: "Shortcuts are paused. Resume shortcuts and test again.",
              });
              return;
            }
            if (action === "pressed" || action === "toggle" || action === "released") {
              const label = shortcutStatus?.hotkey || settingsRef.current?.hotkey || "shortcut";
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
          (payload) => setShortcutStatus(payload as ShortcutStatus),
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
    };
  }, [applyNativeDictation, shortcutStatus?.hotkey, shortcutStatus?.paused]);

  const handleToggleRecording = useCallback(async () => {
    if (busyRef.current) return;
    setBusyState(true);
    try {
      // Browser mock / blocking IPC path (engine events drive Tauri after A3).
      if (recordingRef.current === null) {
        if (hasTauriRuntime()) {
          try {
            await handleDictationAction("start");
            // Native events update phase; keep a soft notice for UX.
            setNotice({ tone: "neutral", message: "Listening…" });
            return;
          } catch {
            // fall through
          }
        }
        const started = await startRecording();
        setRecording(started);
        setRecorderPhase("listening");
        setNotice({
          tone: "success",
          message: `Recording from ${started.microphoneName}.`,
        });
      } else {
        setRecorderPhase("processing");
        setNotice({ tone: "neutral", message: "Transcribing locally…" });
        if (hasTauriRuntime()) {
          try {
            await handleDictationAction("stop");
            await refreshSnapshotOnly();
            setActiveTab("history");
            return;
          } catch {
            // fall through to blocking stop
          }
        }
        const result = await stopRecording();
        await refreshSnapshotOnly();
        setRecording(null);
        setRecorderPhase(result.injection?.injected ? "pasted" : "idle");
        setNotice({
          tone: result.injection?.injected ? "success" : "neutral",
          message: result.injection?.message ?? "Transcript saved to history.",
        });
        setActiveTab("history");
      }
    } catch (error: unknown) {
      setRecording(null);
      setRecorderPhase("error");
      setNotice({ tone: "error", message: stringifyError(error) });
      await refresh().catch(() => undefined);
    } finally {
      setBusyState(false);
    }
  }, [refresh, refreshSnapshotOnly, setBusyState]);

  useEffect(() => {
    if (!micCheck.active) return undefined;
    let cancelled = false;
    const pollLevel = () => {
      getRecordingLevel()
        .then((level) => {
          if (cancelled) return;
          const normalized = Math.max(0, Math.min(1, level));
          const previous = micCheckLevelRef.current;
          const now = window.performance.now();
          const justPassed = normalized > 0.06 && !micCheckPassedRef.current;
          if (
            !justPassed &&
            now - lastMicCheckLevelCommitRef.current < recordingLevelCommitMs &&
            Math.abs(normalized - previous) < recordingLevelDelta
          ) {
            return;
          }
          micCheckLevelRef.current = normalized;
          if (normalized > 0.06) micCheckPassedRef.current = true;
          lastMicCheckLevelCommitRef.current = now;
          setMicCheck((current) => ({
            ...current,
            level: normalized,
            passed: current.passed || normalized > 0.06,
            message:
              normalized > 0.06
                ? "Microphone signal detected."
                : "Listening for microphone signal...",
          }));
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setMicCheck((current) => ({
              ...current,
              message: stringifyError(error),
            }));
          }
        });
    };
    pollLevel();
    const interval = window.setInterval(pollLevel, 250);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [micCheck.active]);

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
      await micCheckStart();
      micCheckLevelRef.current = 0;
      micCheckPassedRef.current = false;
      lastMicCheckLevelCommitRef.current = window.performance.now() - recordingLevelCommitMs;
      setMicCheck({
        active: true,
        passed: false,
        level: 0,
        message: "Listening for microphone signal...",
      });
      setNotice({ tone: "neutral", message: "Microphone check is listening." });
    } catch (error: unknown) {
      // Browser mock / older path: try recording-based check.
      try {
        const started = await startRecording();
        setRecording(started);
        setRecorderPhase("listening");
        setMicCheck({
          active: true,
          passed: false,
          level: 0,
          message: `Listening through ${started.microphoneName}.`,
        });
      } catch (inner: unknown) {
        setMicCheck({
          active: false,
          passed: false,
          level: 0,
          message: stringifyError(inner ?? error),
        });
        setNotice({ tone: "error", message: stringifyError(inner ?? error) });
      }
    } finally {
      setBusyState(false);
    }
  }, [setBusyState]);

  const stopMicCheck = useCallback(async () => {
    try {
      await micCheckStop();
    } catch {
      try {
        await cancelRecording();
      } catch {
        /* ignore */
      }
      setRecording(null);
      setRecorderPhase("idle");
    }
    setMicCheck((current) => ({
      ...current,
      active: false,
      message: current.passed ? "Microphone check passed." : "Microphone check stopped.",
    }));
  }, []);

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

  const needsOnboarding = useMemo(() => {
    if (!snapshot) return false;
    return (
      !snapshot.settings.onboardingComplete ||
      snapshot.settings.onboardingVersion !== ONBOARDING_VERSION
    );
  }, [snapshot]);

  const completeOnboarding = useCallback(async () => {
    if (!settingsDraft) return;
    const nextSettings: AppSettings = {
      ...settingsDraft,
      onboardingComplete: true,
      onboardingVersion: ONBOARDING_VERSION,
    };
    const next = await saveSettings(nextSettings);
    setSnapshot(next);
    setSettingsDraft(next.settings);
    setNotice({ tone: "success", message: "Onboarding complete. Atmospeak is armed." });
  }, [settingsDraft]);

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
        shortcutStatus={shortcutStatus}
        shortcutTest={shortcutTest}
        micCheck={micCheck}
        onStartMicCheck={startMicCheck}
        onStopMicCheck={stopMicCheck}
        onTestShortcut={() => {
          void setShortcutTestActive(true);
          setShortcutTest({
            active: true,
            detected: false,
            message: "Press your dictation shortcut…",
          });
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
        onShowFloatingControl={async () => {
          await showFloatingControl();
        }}
        onComplete={completeOnboarding}
      />
    );
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <Aura size={36} active={recorderPhase === "listening"} />
          <div>
            <strong>Atmospeak</strong>
            <span>Local dictation · CLI ASR</span>
          </div>
        </div>
        <nav aria-label="Atmospeak sections">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                type="button"
                className={clsx("nav-item", activeTab === tab.id && "nav-item--active")}
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={18} />
                {tab.label}
              </button>
            );
          })}
        </nav>
        <StatusLed
          tone={
            recorderPhase === "error"
              ? "hot"
              : recorderPhase === "listening" || recorderPhase === "processing"
                ? "warn"
                : modelStatus?.ready
                  ? "good"
                  : "warn"
          }
          label={notice.message}
        />
      </aside>
      <main className="main-panel">
        {activeTab === "home" ? (
          <HomePanel
            snapshot={snapshot}
            modelStatus={modelStatus}
            recentSession={recentSession}
            lastMetrics={lastMetrics}
            onStart={() => void handleToggleRecording()}
            busy={busy}
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
                id: "",
                phrase: dictEntry.phrase,
                replacement: dictEntry.replacement,
                enabled: true,
                createdAt: new Date().toISOString(),
              });
              setSnapshot(next);
              setDictEntry({ phrase: "", replacement: "" });
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
                id: "",
                trigger: snippetDraft.trigger,
                body: snippetDraft.body,
                enabled: true,
                createdAt: new Date().toISOString(),
              });
              setSnapshot(next);
              setSnippetDraft({ trigger: "", body: "" });
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
            runtimeEvents={runtimeEvents}
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
            onRerunOnboarding={async () => {
              const next = await saveSettings({
                ...settingsDraft,
                onboardingComplete: false,
                onboardingVersion: "",
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
          />
        ) : null}
        {activeTab === "advanced" ? (
          <AdvancedPanel
            settings={settingsDraft}
            setSettings={setSettingsDraft}
            modelStatus={modelStatus}
            modelInventory={modelInventory}
            lastMetrics={lastMetrics}
            onSave={onSaveSettings}
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
    })();
    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);

  useEffect(() => {
    if (phase !== "listening") {
      setLevel(0);
      return undefined;
    }
    const interval = window.setInterval(() => {
      void getRecordingLevel().then((value) => setLevel(value));
    }, recordingLevelPollMs);
    return () => window.clearInterval(interval);
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
      notice={message}
      inputLevel={level}
      inputBands={[]}
      bubbleSize="medium"
      bubbleOpacity={1}
      hostApp={hostApp}
      hotkeyLabel={settings?.hotkey ?? "your shortcut"}
      mode={settings?.mode ?? "pushToTalk"}
      onToggle={() => {
        void handleDictationAction("toggle");
      }}
      onCancel={() => {
        void handleDictationAction("cancel");
      }}
      onMoveStart={() => {
        // Hand the gesture to the OS so the window follows the cursor, then
        // remember where it was dropped.
        if (!hasTauriRuntime()) return;
        const overlay = getCurrentWindow();
        void overlay
          .startDragging()
          .then(() => overlay.outerPosition())
          .then((position) => saveOverlayPosition(position.x, position.y))
          .catch(() => {
            /* window closed mid-drag */
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
