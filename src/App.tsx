import { emit, listen } from "@tauri-apps/api/event";
import { LogicalSize, getCurrentWindow } from "@tauri-apps/api/window";
import clsx from "clsx";
import {
  BookOpen,
  CheckCircle2,
  Clipboard,
  Copy,
  Cpu,
  Database,
  Download,
  History,
  Keyboard,
  Mic,
  Radio,
  RotateCw,
  Scissors,
  Settings,
  Sparkles,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { FormEvent } from "react";
import "./App.css";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { DictionaryPanel } from "./components/DictionaryPanel";
import { EmptyState } from "./components/EmptyState";
import { HomePanel } from "./components/HomePanel";
import { PanelTitle } from "./components/PanelTitle";
import { RecorderOverlay } from "./components/RecorderOverlay";
import { SnippetPanel } from "./components/SnippetPanel";
import { ToggleRow } from "./components/ToggleRow";
import type {
  RecorderOverlaySize,
  RecorderPhase,
  RecorderResizeDirection,
} from "./components/RecorderOverlay";
import { StatusLed } from "./components/StatusLed";
import {
  cancelRecording,
  copyText,
  deleteDictionaryEntry,
  deleteSnippet,
  exportSession,
  getAppSnapshot,
  checkForUpdates,
  downloadAndInstallUpdate,
  getModelInventory,
  hasTauriRuntime,
  getModelStatus,
  getRecordingFftBands,
  getRecordingLevel,
  getRuntimeEvents,
  getShortcutStatus,
  handleDictationAction,
  injectText,
  listMicrophones,
  listRecentApps,
  polishSession,
  saveSettings,
  searchSessions,
  showMainWindow,
  setShortcutTestActive,
  setShortcutsPaused,
  showFloatingControl,
  startRecording,
  stopRecording,
  submitFeedback,
  updateSessionNotes,
  upsertDictionaryEntry,
  upsertSnippet,
} from "./lib/api";
import type {
  AppNotice,
  AppSettings,
  AppSnapshot,
  DictionaryEntry,
  ExportFormat,
  HistorySearchFilters,
  HubTab,
  MicrophoneInfo,
  RecentAppUsage,
  ModelInventory,
  ModelStatus,
  NativeDictationEvent,
  PolishStyle,
  RecordingStarted,
  RuntimeEvent,
  ShortcutStatus,
  Snippet,
  TranscriptStreamEvent,
  TranscriptSession,
  UpdateCheckResult,
  UpdateStatus,
} from "./types/dictation";

const onboardingVersion = "desktop-runtime-parity-v1";
const recordingLevelPollMs = 250;
const recordingFftPollMs = 60;
const recordingLevelCommitMs = 400;
const recordingLevelDelta = 0.03;
const overlaySizeOrder: RecorderOverlaySize[] = ["compact", "standard", "expanded"];
const overlaySizeDimensions: Record<RecorderOverlaySize, { width: number; height: number }> = {
  compact: { width: 360, height: 92 },
  standard: { width: 560, height: 220 },
  expanded: { width: 680, height: 300 },
};
const shortcutOptions = [
  "Ctrl+Alt+D",
  "Ctrl+Alt+Space",
  "Ctrl+Shift+Space",
  "Ctrl+Win",
  "Ctrl+Win+Space",
];
const languageOptions = [
  { value: "auto", label: "Auto-detect" },
  { value: "en", label: "English" },
  { value: "es", label: "Spanish" },
  { value: "fr", label: "French" },
  { value: "de", label: "German" },
  { value: "it", label: "Italian" },
  { value: "pt", label: "Portuguese" },
  { value: "nl", label: "Dutch" },
  { value: "pl", label: "Polish" },
  { value: "ja", label: "Japanese" },
  { value: "ko", label: "Korean" },
  { value: "zh", label: "Chinese" },
];
const polishStyleOptions: Array<{ value: PolishStyle; label: string }> = [
  { value: "concise", label: "Concise" },
  { value: "formal", label: "Formal" },
  { value: "casual", label: "Casual" },
  { value: "excited", label: "Excited" },
  { value: "summarize", label: "Summarize" },
];

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

interface LiveTranscriptState {
  sessionId: string | null;
  phase: "idle" | "partial" | "stable" | "final" | "error";
  text: string;
  latencyMs: number | null;
}

const emptyLiveTranscript: LiveTranscriptState = {
  sessionId: null,
  phase: "idle",
  text: "",
  latencyMs: null,
};

function App() {
  const isOverlayView =
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).get("view") === "overlay";

  if (isOverlayView) {
    return <OverlayWindow />;
  }

  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<AppSettings | null>(null);
  const [shortcutStatus, setShortcutStatus] = useState<ShortcutStatus | null>(null);
  const [microphones, setMicrophones] = useState<MicrophoneInfo[]>([]);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [modelInventory, setModelInventory] = useState<ModelInventory | null>(null);
  const [runtimeEvents, setRuntimeEvents] = useState<RuntimeEvent[]>([]);
  const [recentApps, setRecentApps] = useState<RecentAppUsage[]>([]);
  const [activeTab, setActiveTab] = useState<HubTab>("home");
  const [recording, setRecording] = useState<RecordingStarted | null>(null);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [recorderPhase, setRecorderPhase] = useState<RecorderPhase>("idle");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<AppNotice>({
    tone: "neutral",
    message: "Wind Speak is standing by.",
  });
  const [liveTranscript, setLiveTranscript] = useState<LiveTranscriptState>(emptyLiveTranscript);
  const [shortcutTest, setShortcutTest] = useState<ShortcutTestState>({
    active: false,
    detected: false,
    message: "Shortcut test is idle.",
  });
  const [pasteTest, setPasteTest] = useState<PasteTestState>({
    running: false,
    passed: false,
    message: "Paste test has not run yet.",
  });
  const [micCheck, setMicCheck] = useState<MicCheckState>({
    active: false,
    passed: false,
    level: 0,
    message: "Microphone check has not run yet.",
  });
  const [dictionaryDraft, setDictionaryDraft] = useState({ phrase: "", replacement: "" });
  const [snippetDraft, setSnippetDraft] = useState({ trigger: "", body: "" });
  const [feedbackDraft, setFeedbackDraft] = useState("");
  const [polishingSessionId, setPolishingSessionId] = useState<string | null>(null);
  const recordingRef = useRef<RecordingStarted | null>(null);
  const onboardingOverlayShownRef = useRef(false);
  const busyRef = useRef(false);
  const startingRecordingRef = useRef(false);
  const queuedPushToTalkReleaseRef = useRef(false);
  const settingsRef = useRef<AppSettings | null>(null);
  const shortcutTestRef = useRef(shortcutTest);
  const micCheckLevelRef = useRef(0);
  const micCheckPassedRef = useRef(false);
  const lastMicCheckLevelCommitRef = useRef(0);

  const refresh = useCallback(async () => {
    const [
      nextSnapshot,
      nextMicrophones,
      nextModelStatus,
      nextModelInventory,
      nextShortcutStatus,
      nextRuntimeEvents,
    ] = await Promise.all([
      getAppSnapshot(),
      listMicrophones(),
      getModelStatus(),
      getModelInventory(),
      getShortcutStatus(),
      getRuntimeEvents(),
    ]);
    setSnapshot(nextSnapshot);
    setSettingsDraft(nextSnapshot.settings);
    setMicrophones(nextMicrophones);
    setModelStatus(nextModelStatus);
    setModelInventory(nextModelInventory);
    setShortcutStatus(nextShortcutStatus);
    setRuntimeEvents(nextRuntimeEvents);
    if (hasTauriRuntime() || nextSnapshot.sessions.length > 0) {
      setRecentApps(await listRecentApps(8));
    }
  }, []);

  const refreshSnapshotOnly = useCallback(async () => {
    const nextSnapshot = await getAppSnapshot();
    setSnapshot(nextSnapshot);
    setSettingsDraft(nextSnapshot.settings);
    if (hasTauriRuntime() || nextSnapshot.sessions.length > 0) {
      const nextApps = await listRecentApps(8);
      setRecentApps(nextApps);
    }
  }, []);

  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  useEffect(() => {
    settingsRef.current = settingsDraft;
  }, [settingsDraft]);

  useEffect(() => {
    shortcutTestRef.current = shortcutTest;
  }, [shortcutTest]);

  useEffect(() => {
    micCheckLevelRef.current = micCheck.level;
    micCheckPassedRef.current = micCheck.passed;
  }, [micCheck.level, micCheck.passed]);

  useEffect(() => {
    refresh().catch((error: unknown) => {
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

  const finishRecording = useCallback(async () => {
    setRecording(null);
    setRecorderPhase("processing");
    setNotice({ tone: "neutral", message: "Transcribing locally..." });
    const result = await stopRecording();
    await refreshSnapshotOnly();
    setRecorderPhase(result.injection?.injected ? "pasted" : "idle");
    setNotice({
      tone: result.injection?.injected ? "success" : "neutral",
      message: result.injection?.message ?? "Transcript saved to history.",
    });
    setActiveTab("history");
  }, [refreshSnapshotOnly]);

  const setBusyState = useCallback((nextBusy: boolean) => {
    busyRef.current = nextBusy;
    setBusy(nextBusy);
  }, []);

  const handleToggleRecording = useCallback(async () => {
    if (busyRef.current) {
      return;
    }

    setBusyState(true);
    try {
      if (recordingRef.current === null) {
        queuedPushToTalkReleaseRef.current = false;
        startingRecordingRef.current = true;
        const started = await startRecording();
        startingRecordingRef.current = false;
        setRecording(started);
        setRecorderPhase("listening");
        setNotice({ tone: "success", message: `Recording from ${started.microphoneName}.` });
        if (queuedPushToTalkReleaseRef.current) {
          queuedPushToTalkReleaseRef.current = false;
          await finishRecording();
        }
      } else {
        await finishRecording();
      }
    } catch (error: unknown) {
      setRecording(null);
      setRecorderPhase("error");
      startingRecordingRef.current = false;
      queuedPushToTalkReleaseRef.current = false;
      await refresh().catch(() => undefined);
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      startingRecordingRef.current = false;
      setBusyState(false);
    }
  }, [finishRecording, refresh, setBusyState]);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    const payload = {
      recording,
      elapsedSeconds,
      busy,
      recorderPhase,
      modelStatus,
      shortcutStatus,
      notice,
      recordingLevel: 0,
      recordingBands: [],
      liveTranscript,
    };
    void emit("wind-speak://dictation-state", payload);
    return undefined;
  }, [
    busy,
    elapsedSeconds,
    liveTranscript,
    modelStatus,
    notice,
    recorderPhase,
    recording,
    shortcutStatus,
  ]);

  const handleCancel = useCallback(async () => {
    if (busyRef.current) {
      return;
    }

    setBusyState(true);
    try {
      queuedPushToTalkReleaseRef.current = false;
      await cancelRecording();
      setRecording(null);
      setRecorderPhase("idle");
      setNotice({ tone: "neutral", message: "Recording cancelled." });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusyState(false);
    }
  }, [setBusyState]);

  useEffect(() => {
    if (!micCheck.active) {
      return undefined;
    }

    let cancelled = false;
    const pollLevel = () => {
      getRecordingLevel()
        .then((level) => {
          if (cancelled) {
            return;
          }
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
          if (normalized > 0.06) {
            micCheckPassedRef.current = true;
          }
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
      const started = await startRecording();
      micCheckLevelRef.current = 0;
      setRecording(started);
      setRecorderPhase("listening");
      setMicCheck({
        active: true,
        passed: false,
        level: 0,
        message: `Listening through ${started.microphoneName}.`,
      });
      micCheckPassedRef.current = false;
      lastMicCheckLevelCommitRef.current = window.performance.now() - recordingLevelCommitMs;
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
    setBusyState(true);
    try {
      await cancelRecording();
      const passed = micCheckLevelRef.current > 0.06 || micCheck.passed;
      setRecording(null);
      setRecorderPhase("idle");
      setMicCheck((current) => ({
        ...current,
        active: false,
        passed,
        level: passed ? Math.max(current.level, 0.16) : 0,
        message: passed ? "Microphone check passed." : "No microphone signal detected.",
      }));
      setNotice({
        tone: passed ? "success" : "warning",
        message: passed ? "Microphone check passed." : "No microphone signal detected.",
      });
    } catch (error: unknown) {
      setMicCheck((current) => ({
        ...current,
        active: false,
        message: stringifyError(error),
      }));
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusyState(false);
    }
  }, [micCheck.passed, setBusyState]);

  const armShortcutTest = useCallback(() => {
    if (shortcutStatus?.paused) {
      setShortcutTest({
        active: false,
        detected: false,
        message: "Shortcuts are paused. Resume shortcuts and test again.",
      });
      setNotice({ tone: "warning", message: "Shortcuts are paused." });
      return;
    }

    const label = shortcutStatus?.hotkey || settingsRef.current?.hotkey || "the active shortcut";
    void setShortcutTestActive(true);
    setShortcutTest({
      active: true,
      detected: false,
      message: `Press ${label} now.`,
    });
    setNotice({ tone: "neutral", message: `Listening for ${label}.` });
    window.setTimeout(() => {
      setShortcutTest((current) =>
        current.active && !current.detected
          ? {
              active: false,
              detected: false,
              message: "No shortcut press detected. Choose another shortcut or use the floating control.",
            }
          : current,
      );
      void setShortcutTestActive(false);
    }, 8000);
  }, [shortcutStatus?.hotkey, shortcutStatus?.paused]);

  const runPasteTest = useCallback(async () => {
    setPasteTest({
      running: true,
      passed: false,
      message: "Focus the target text field. Pasting in 3...",
    });
    setNotice({ tone: "neutral", message: "Focus the target app. Wind Speak will paste in 3 seconds." });
    try {
      for (const seconds of [2, 1]) {
        await new Promise((resolve) => window.setTimeout(resolve, 1000));
        setPasteTest({
          running: true,
          passed: false,
          message: `Focus the target text field. Pasting in ${seconds}...`,
        });
      }
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
      const result = await injectText("Wind Speak paste test");
      setPasteTest({
        running: false,
        passed: result.injected,
        message: result.message,
      });
      setNotice({
        tone: result.injected ? "success" : "warning",
        message: result.message,
      });
    } catch (error: unknown) {
      const message = stringifyError(error);
      setPasteTest({ running: false, passed: false, message });
      setNotice({ tone: "error", message });
    }
  }, []);

  const handleShowFloatingControl = useCallback(async () => {
    try {
      await showFloatingControl();
      setNotice({
        tone: "success",
        message: "Floating control shown and reset above other windows.",
      });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    }
  }, []);

  const handleNativeDictationEvent = useCallback(
    (payload: NativeDictationEvent) => {
      queuedPushToTalkReleaseRef.current = false;
      startingRecordingRef.current = false;
      setRecording(payload.recording);
      setRecorderPhase(payload.phase);
      setBusyState(payload.phase === "processing");
      if (payload.phase === "listening" && payload.recording !== null) {
        setLiveTranscript({
          sessionId: payload.recording.id,
          phase: "partial",
          text: "",
          latencyMs: null,
        });
      }
      if (payload.phase === "idle" || payload.phase === "error") {
        setLiveTranscript(emptyLiveTranscript);
      }
      setNotice({
        tone:
          payload.phase === "error"
            ? "error"
            : payload.phase === "pasted"
              ? "success"
              : "neutral",
        message: payload.message,
      });
      if (payload.result !== null) {
        void refreshSnapshotOnly();
        setActiveTab("history");
      }
    },
    [refreshSnapshotOnly, setBusyState],
  );

  const handleTranscriptStreamEvent = useCallback((payload: TranscriptStreamEvent) => {
    setLiveTranscript((current) => {
      if (current.sessionId !== null && current.sessionId !== payload.sessionId) {
        return current;
      }
      const text = payload.stableText || payload.provisionalText || payload.message || "";
      return {
        sessionId: payload.sessionId,
        phase: payload.phase,
        text,
        latencyMs: payload.latencyMs,
      };
    });
  }, []);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    let removeShortcutListener: (() => void) | undefined;
    let removeOverlayListener: (() => void) | undefined;
    let removeShortcutStatusListener: (() => void) | undefined;
    let removeOverlayVisibilityListener: (() => void) | undefined;
    let removeNativeDictationListener: (() => void) | undefined;
    let removeRuntimeEventListener: (() => void) | undefined;
    let removeTranscriptPartialListener: (() => void) | undefined;
    let removeTranscriptStableListener: (() => void) | undefined;
    let removeTranscriptFinalListener: (() => void) | undefined;
    listen<string>("wind-speak://shortcut", (event) => {
      const action = event.payload;
      if (shortcutTestRef.current.active) {
        if (shortcutStatus?.paused) {
          setShortcutTest({
            active: false,
            detected: false,
            message: "Shortcuts are paused. Resume shortcuts and test again.",
          });
          setNotice({ tone: "warning", message: "Shortcuts are paused." });
          return;
        }
        if (action === "pressed" || action === "toggle") {
          const label = shortcutStatus?.hotkey || settingsRef.current?.hotkey || "shortcut";
          void setShortcutTestActive(false);
          setShortcutTest({
            active: false,
            detected: true,
            message: `${label} detected by the desktop runtime.`,
          });
          setNotice({ tone: "success", message: `${label} detected.` });
        }
        return;
      }
    })
      .then((unlisten) => {
        removeShortcutListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<string>("wind-speak://overlay-command", (event) => {
      if (event.payload === "toggle") {
        void handleDictationAction("toggle");
      }
      if (event.payload === "cancel") {
        void handleDictationAction("cancel");
      }
    })
      .then((unlisten) => {
        removeOverlayListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<ShortcutStatus>("wind-speak://shortcut-status", (event) => {
      setShortcutStatus(event.payload);
      setNotice({
        tone: event.payload.registered ? "success" : "warning",
        message: event.payload.message,
      });
    })
      .then((unlisten) => {
        removeShortcutStatusListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<NativeDictationEvent>("wind-speak://native-dictation", (event) => {
      handleNativeDictationEvent(event.payload);
    })
      .then((unlisten) => {
        removeNativeDictationListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<RuntimeEvent>("wind-speak://runtime-event", (event) => {
      setRuntimeEvents((current) => [event.payload, ...current].slice(0, 30));
    })
      .then((unlisten) => {
        removeRuntimeEventListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<TranscriptStreamEvent>("wind-speak://transcript-partial", (event) => {
      handleTranscriptStreamEvent(event.payload);
    })
      .then((unlisten) => {
        removeTranscriptPartialListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<TranscriptStreamEvent>("wind-speak://transcript-stable", (event) => {
      handleTranscriptStreamEvent(event.payload);
    })
      .then((unlisten) => {
        removeTranscriptStableListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<TranscriptStreamEvent>("wind-speak://transcript-final", (event) => {
      handleTranscriptStreamEvent(event.payload);
    })
      .then((unlisten) => {
        removeTranscriptFinalListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    listen<string>("wind-speak://overlay-visibility", (event) => {
      setNotice({ tone: "neutral", message: event.payload });
    })
      .then((unlisten) => {
        removeOverlayVisibilityListener = unlisten;
      })
      .catch((error: unknown) => {
        setNotice({ tone: "warning", message: stringifyError(error) });
      });

    return () => {
      removeShortcutListener?.();
      removeOverlayListener?.();
      removeShortcutStatusListener?.();
      removeOverlayVisibilityListener?.();
      removeNativeDictationListener?.();
      removeRuntimeEventListener?.();
      removeTranscriptPartialListener?.();
      removeTranscriptStableListener?.();
      removeTranscriptFinalListener?.();
    };
  }, [handleNativeDictationEvent, handleTranscriptStreamEvent, shortcutStatus?.hotkey]);

  const handleSaveSettings = async () => {
    if (settingsDraft === null) {
      return;
    }

    setBusyState(true);
    try {
      const nextSnapshot = await saveSettings(settingsDraft);
      setSnapshot(nextSnapshot);
      setSettingsDraft(nextSnapshot.settings);
      setModelStatus(await getModelStatus());
      setModelInventory(await getModelInventory());
      setShortcutStatus(await getShortcutStatus());
      setNotice({ tone: "success", message: "Settings saved." });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusyState(false);
    }
  };

  const rerunOnboarding = async () => {
    if (settingsDraft === null) {
      return;
    }

    setBusyState(true);
    try {
      const nextSettings = {
        ...settingsDraft,
        onboardingComplete: false,
        onboardingVersion: "",
      };
      const nextSnapshot = await saveSettings(nextSettings);
      setSnapshot(nextSnapshot);
      setSettingsDraft(nextSnapshot.settings);
      setPasteTest({
        running: false,
        passed: false,
        message: "Paste test has not run yet.",
      });
      setShortcutTest({
        active: false,
        detected: false,
        message: "Shortcut test is idle.",
      });
      setNotice({ tone: "neutral", message: "Onboarding restarted." });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusyState(false);
    }
  };

  const addDictionaryEntry = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const phrase = dictionaryDraft.phrase.trim();
    const replacement = dictionaryDraft.replacement.trim();
    if (phrase.length === 0 || replacement.length === 0) {
      setNotice({ tone: "warning", message: "Dictionary entries need both fields." });
      return;
    }

    const entry: DictionaryEntry = {
      id: "",
      phrase,
      replacement,
      enabled: true,
      createdAt: new Date().toISOString(),
    };
    const nextSnapshot = await upsertDictionaryEntry(entry);
    setSnapshot(nextSnapshot);
    setDictionaryDraft({ phrase: "", replacement: "" });
    setNotice({ tone: "success", message: "Dictionary entry added." });
  };

  const addSnippet = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trigger = snippetDraft.trigger.trim();
    const body = snippetDraft.body.trim();
    if (trigger.length === 0 || body.length === 0) {
      setNotice({ tone: "warning", message: "Snippets need a trigger and body." });
      return;
    }

    const snippet: Snippet = {
      id: "",
      trigger,
      body,
      enabled: true,
      createdAt: new Date().toISOString(),
    };
    const nextSnapshot = await upsertSnippet(snippet);
    setSnapshot(nextSnapshot);
    setSnippetDraft({ trigger: "", body: "" });
    setNotice({ tone: "success", message: "Snippet added." });
  };

  const handlePolishSession = async (session: TranscriptSession) => {
    setPolishingSessionId(session.id);
    try {
      const result = await polishSession(session.id);
      setSnapshot(result.snapshot);
      setSettingsDraft(result.snapshot.settings);
      setNotice({
        tone: result.polish.changed ? "success" : "neutral",
        message: result.polish.changed
          ? `AI edit applied with ${result.polish.style} style.`
          : "AI edit completed with no changes.",
      });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setPolishingSessionId(null);
    }
  };

  const recentSession = snapshot?.sessions[0] ?? null;
  const readiness = useMemo(() => {
    if (modelStatus?.ready) {
      return { tone: "good" as const, label: "Offline engine ready" };
    }
    return { tone: "warn" as const, label: "Runtime incomplete" };
  }, [modelStatus]);
  const needsOnboarding =
    snapshot !== null &&
    settingsDraft !== null &&
    (!settingsDraft.onboardingComplete || settingsDraft.onboardingVersion !== onboardingVersion);

  useEffect(() => {
    if (!needsOnboarding || !hasTauriRuntime() || onboardingOverlayShownRef.current) {
      return;
    }

    onboardingOverlayShownRef.current = true;
    showFloatingControl().catch((error: unknown) => {
      setNotice({ tone: "warning", message: stringifyError(error) });
    });
  }, [needsOnboarding]);

  if (snapshot === null || settingsDraft === null) {
    return (
      <main className="boot">
        <div className="boot__mark">WS</div>
        <p>Starting local command surface...</p>
      </main>
    );
  }

  if (needsOnboarding) {
    return (
      <Onboarding
        settings={settingsDraft}
        setSettings={setSettingsDraft}
        microphones={microphones}
        modelStatus={modelStatus}
        shortcutStatus={shortcutStatus}
        shortcutTest={shortcutTest}
        micCheck={micCheck}
        onStartMicCheck={startMicCheck}
        onStopMicCheck={stopMicCheck}
        onTestShortcut={armShortcutTest}
        pasteTest={pasteTest}
        onPasteTest={runPasteTest}
        onShowFloatingControl={handleShowFloatingControl}
        onComplete={async () => {
          const nextSettings = {
            ...settingsDraft,
            onboardingComplete: true,
            onboardingVersion,
          };
          const nextSnapshot = await saveSettings(nextSettings);
          const nextShortcutStatus = await getShortcutStatus();
          setSnapshot(nextSnapshot);
          setSettingsDraft(nextSnapshot.settings);
          setShortcutStatus(nextShortcutStatus);
          setNotice({ tone: "success", message: "Onboarding complete. Wind Speak is armed." });
        }}
      />
    );
  }

  return (
    <ErrorBoundary>
    <main className="app-shell">
      <section className="top-strip" aria-label="Application status">
        <div className="brand-block">
          <span className="brand-block__index">0001</span>
          <div>
            <p className="eyebrow">Wind Speak</p>
            <h1>Local dictation console</h1>
          </div>
        </div>
        <div className="status-row">
          <StatusLed tone={readiness.tone} label={readiness.label} />
          <StatusLed tone={recording ? "hot" : "idle"} label={recording ? "Recording" : "Idle"} />
          <StatusLed
            tone={shortcutStatus?.paused ? "warn" : shortcutStatus?.registered ? "good" : "warn"}
            label={
              shortcutStatus?.paused
                ? "Shortcuts paused"
                : shortcutStatus?.hotkey || "Shortcut unavailable"
            }
          />
        </div>
      </section>

      <RecorderOverlay
        recording={recording}
        elapsedSeconds={elapsedSeconds}
        busy={busy}
        phase={recorderPhase}
        modelStatus={modelStatus}
        hotkeyLabel={shortcutStatus?.hotkey || snapshot.settings.hotkey}
        notice={shortcutStatus?.registered ? undefined : shortcutStatus?.message}
        liveTranscript={liveTranscript}
        inputLevel={0}
        bubbleOpacity={snapshot.settings.bubbleOpacity}
        bubbleSize={snapshot.settings.bubbleSize}
        onToggle={handleToggleRecording}
        onCancel={handleCancel}
      />

      <section className="notice-rail" aria-live="polite">
        <span className={clsx("notice-rail__tone", `notice-rail__tone--${notice.tone}`)} />
        <p>{notice.message}</p>
      </section>

      <section className="workspace">
        <nav className="side-nav" aria-label="Hub sections">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                className={clsx("side-nav__item", activeTab === tab.id && "is-active")}
                type="button"
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={18} />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="hub">
          {activeTab === "home" && (
            <HomePanel
              snapshot={snapshot}
              modelStatus={modelStatus}
              recentSession={recentSession}
              onStart={handleToggleRecording}
              onPolishLatest={handlePolishSession}
              polishingSessionId={polishingSessionId}
              onUpdatePrivacy={async (privacyMode, autoDeleteTranscriptsAfterMinutes) => {
                const nextSettings = {
                  ...snapshot.settings,
                  privacyMode,
                  autoDeleteTranscriptsAfterMinutes,
                };
                const nextSnapshot = await saveSettings(nextSettings);
                setSnapshot(nextSnapshot);
                setSettingsDraft(nextSnapshot.settings);
                setNotice({
                  tone: "success",
                  message: privacyMode
                    ? "Privacy mode enabled."
                    : "Privacy mode disabled.",
                });
              }}
              busy={busy}
            />
          )}
          {activeTab === "history" && (
            <HistoryPanel
              sessions={snapshot.sessions}
              recentApps={recentApps}
              onCopy={async (session) => {
                const message = await copyText(session.cleanedText);
                setNotice({ tone: "success", message });
              }}
              onInject={async (session) => {
                const result = await injectText(session.cleanedText);
                setRecorderPhase(result.injected ? "pasted" : "idle");
                setNotice({ tone: "success", message: result.message });
              }}
              onPolish={handlePolishSession}
              polishingSessionId={polishingSessionId}
              onExport={async (session, format) => {
                try {
                  const content = await exportSession(session.id, format);
                  const blob = new Blob([content], { type: "text/plain" });
                  const url = URL.createObjectURL(blob);
                  const link = document.createElement("a");
                  link.href = url;
                  link.download = `wind-speak-${session.id}.${format}`;
                  link.click();
                  URL.revokeObjectURL(url);
                  setNotice({
                    tone: "success",
                    message: `Exported transcript as ${format.toUpperCase()}.`,
                  });
                } catch (error) {
                  setNotice({
                    tone: "error",
                    message: `Export failed: ${(error as Error).message}`,
                  });
                }
              }}
              onUpdateNotes={async (session, notes) => {
                setSnapshot(await updateSessionNotes(session.id, notes));
              }}
            />
          )}
          {activeTab === "dictionary" && (
            <DictionaryPanel
              entries={snapshot.dictionary}
              draft={dictionaryDraft}
              setDraft={setDictionaryDraft}
              onSubmit={addDictionaryEntry}
              onToggle={async (entry) => {
                setSnapshot(await upsertDictionaryEntry({ ...entry, enabled: !entry.enabled }));
              }}
              onDelete={async (entry) => {
                setSnapshot(await deleteDictionaryEntry(entry.id));
              }}
            />
          )}
          {activeTab === "snippets" && (
            <SnippetPanel
              snippets={snapshot.snippets}
              draft={snippetDraft}
              setDraft={setSnippetDraft}
              onSubmit={addSnippet}
              onToggle={async (snippet) => {
                setSnapshot(await upsertSnippet({ ...snippet, enabled: !snippet.enabled }));
              }}
              onDelete={async (snippet) => {
                setSnapshot(await deleteSnippet(snippet.id));
              }}
            />
          )}
          {activeTab === "advanced" && (
            <AdvancedPanel
              settings={settingsDraft}
              setSettings={setSettingsDraft}
              modelStatus={modelStatus}
              modelInventory={modelInventory}
              onSave={handleSaveSettings}
            />
          )}
          {activeTab === "settings" && (
            <SettingsPanel
              settings={settingsDraft}
              setSettings={setSettingsDraft}
              microphones={microphones}
              shortcutStatus={shortcutStatus}
              shortcutTest={shortcutTest}
              runtimeEvents={runtimeEvents}
              onTestShortcut={armShortcutTest}
              onToggleShortcutsPaused={async () => {
                const nextStatus = await setShortcutsPaused(!shortcutStatus?.paused);
                setShortcutStatus(nextStatus);
                setNotice({
                  tone: nextStatus.paused ? "warning" : "success",
                  message: nextStatus.message,
                });
              }}
              onShowFloatingControl={handleShowFloatingControl}
              onRerunOnboarding={rerunOnboarding}
              onSave={handleSaveSettings}
              updateStatus={updateStatus}
              updateResult={updateResult}
              onCheckUpdates={async () => {
                setUpdateStatus("checking");
                try {
                  const result = await checkForUpdates();
                  setUpdateResult(result);
                  setUpdateStatus(result.available ? "available" : "current");
                  setNotice({ tone: result.available ? "warning" : "success", message: result.message });
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
                  setNotice({ tone: "success", message: result.message });
                } catch (error: unknown) {
                  setUpdateStatus("error");
                  setNotice({ tone: "error", message: stringifyError(error) });
                }
              }}
              feedbackDraft={feedbackDraft}
              setFeedbackDraft={setFeedbackDraft}
              onSubmitFeedback={async () => {
                try {
                  const result = await submitFeedback(feedbackDraft);
                  setFeedbackDraft("");
                  setNotice({
                    tone: result.delivered ? "success" : "neutral",
                    message: result.message,
                  });
                  setRuntimeEvents(await getRuntimeEvents());
                } catch (error: unknown) {
                  setNotice({ tone: "error", message: stringifyError(error) });
                }
              }}
            />
          )}
        </div>
      </section>
    </main>
    </ErrorBoundary>
  );
}

interface OverlayStatePayload {
  recording: RecordingStarted | null;
  elapsedSeconds: number;
  busy: boolean;
  recorderPhase: RecorderPhase;
  modelStatus: ModelStatus | null;
  shortcutStatus: ShortcutStatus | null;
  notice: AppNotice;
  recordingLevel: number;
  recordingBands: number[];
  liveTranscript: LiveTranscriptState;
}

function OverlayWindow() {
  const [overlaySize, setOverlaySize] = useState<RecorderOverlaySize>(() => {
    if (typeof window === "undefined") {
      return "standard";
    }
    const saved = window.localStorage.getItem("wind-speak-overlay-size");
    return overlaySizeOrder.includes(saved as RecorderOverlaySize)
      ? (saved as RecorderOverlaySize)
      : "standard";
  });
  const [state, setState] = useState<OverlayStatePayload>({
    recording: null,
    elapsedSeconds: 0,
    busy: false,
    recorderPhase: "idle",
    modelStatus: null,
    shortcutStatus: null,
    notice: { tone: "neutral", message: "Wind Speak is standing by." },
    recordingLevel: 0,
    recordingBands: [],
    liveTranscript: emptyLiveTranscript,
  });
  const recordingLevelRef = useRef(0);
  const lastRecordingLevelCommitRef = useRef(0);

  useEffect(() => {
    document.body.classList.add("is-overlay-window");
    return () => document.body.classList.remove("is-overlay-window");
  }, []);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return;
    }
    const size = overlaySizeDimensions[overlaySize];
    window.localStorage.setItem("wind-speak-overlay-size", overlaySize);
    getCurrentWindow()
      .setSize(new LogicalSize(size.width, size.height))
      .catch(() => undefined);
  }, [overlaySize]);

  const handleMoveStart = useCallback(() => {
    if (!hasTauriRuntime()) {
      return;
    }
    getCurrentWindow().startDragging().catch(() => undefined);
  }, []);

  const handleResizeStart = useCallback((direction: RecorderResizeDirection) => {
    if (!hasTauriRuntime()) {
      return;
    }
    getCurrentWindow().startResizeDragging(direction).catch(() => undefined);
  }, []);

  const handleCycleOverlaySize = useCallback(() => {
    setOverlaySize((current) => {
      const currentIndex = overlaySizeOrder.indexOf(current);
      return overlaySizeOrder[(currentIndex + 1) % overlaySizeOrder.length];
    });
  }, []);

  const handleOpenHub = useCallback(() => {
    showMainWindow().catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    let removeStateListener: (() => void) | undefined;
    let removeNativeDictationListener: (() => void) | undefined;
    let removeTranscriptPartialListener: (() => void) | undefined;
    let removeTranscriptStableListener: (() => void) | undefined;
    let removeTranscriptFinalListener: (() => void) | undefined;
    listen<OverlayStatePayload>("wind-speak://dictation-state", (event) => {
      setState((current) => ({
        ...event.payload,
        elapsedSeconds: event.payload.recording === null ? 0 : current.elapsedSeconds,
        recordingLevel: event.payload.recording === null ? 0 : current.recordingLevel,
        recordingBands: event.payload.recording === null ? [] : current.recordingBands,
        liveTranscript:
          event.payload.recording === null ? emptyLiveTranscript : current.liveTranscript,
      }));
    })
      .then((unlisten) => {
        removeStateListener = unlisten;
      })
      .catch(() => undefined);

    listen<NativeDictationEvent>("wind-speak://native-dictation", (event) => {
      const payload = event.payload;
      setState((current) => ({
        ...current,
        recording: payload.recording,
        busy: payload.phase === "processing",
        recorderPhase: payload.phase,
        liveTranscript:
          payload.phase === "listening" && payload.recording !== null
            ? {
                sessionId: payload.recording.id,
                phase: "partial",
                text: "",
                latencyMs: null,
              }
            : payload.phase === "idle" || payload.phase === "error"
              ? emptyLiveTranscript
              : current.liveTranscript,
        notice: {
          tone: payload.phase === "error" ? "error" : payload.phase === "pasted" ? "success" : "neutral",
          message: payload.message,
        },
      }));
    })
      .then((unlisten) => {
        removeNativeDictationListener = unlisten;
      })
      .catch(() => undefined);

    const handleTranscriptEvent = (payload: TranscriptStreamEvent) => {
      setState((current) => {
        if (
          current.liveTranscript.sessionId !== null &&
          current.liveTranscript.sessionId !== payload.sessionId
        ) {
          return current;
        }
        return {
          ...current,
          liveTranscript: {
            sessionId: payload.sessionId,
            phase: payload.phase,
            text: payload.stableText || payload.provisionalText || payload.message || "",
            latencyMs: payload.latencyMs,
          },
        };
      });
    };

    listen<TranscriptStreamEvent>("wind-speak://transcript-partial", (event) => {
      handleTranscriptEvent(event.payload);
    })
      .then((unlisten) => {
        removeTranscriptPartialListener = unlisten;
      })
      .catch(() => undefined);

    listen<TranscriptStreamEvent>("wind-speak://transcript-stable", (event) => {
      handleTranscriptEvent(event.payload);
    })
      .then((unlisten) => {
        removeTranscriptStableListener = unlisten;
      })
      .catch(() => undefined);

    listen<TranscriptStreamEvent>("wind-speak://transcript-final", (event) => {
      handleTranscriptEvent(event.payload);
    })
      .then((unlisten) => {
        removeTranscriptFinalListener = unlisten;
      })
      .catch(() => undefined);

    return () => {
      removeStateListener?.();
      removeNativeDictationListener?.();
      removeTranscriptPartialListener?.();
      removeTranscriptStableListener?.();
      removeTranscriptFinalListener?.();
    };
  }, []);

  useEffect(() => {
    if (state.recording === null) {
      setState((current) =>
        current.elapsedSeconds === 0 &&
          current.recordingLevel === 0 &&
          current.recordingBands.length === 0
          ? current
          : { ...current, elapsedSeconds: 0, recordingLevel: 0, recordingBands: [] },
      );
      recordingLevelRef.current = 0;
      lastRecordingLevelCommitRef.current = 0;
      return undefined;
    }

    const startedAt = new Date(state.recording.startedAt).getTime();
    const interval = window.setInterval(() => {
      setState((current) => ({
        ...current,
        elapsedSeconds: Math.max(0, (Date.now() - startedAt) / 1000),
      }));
    }, 1000);

    return () => window.clearInterval(interval);
  }, [state.recording]);

  useEffect(() => {
    if (state.recording === null) {
      return undefined;
    }

    let cancelled = false;
    const pollBands = () => {
      getRecordingFftBands()
        .then((bands) => {
          if (!cancelled) {
            setState((current) => ({
              ...current,
              recordingBands: bands.slice(0, 7).map((band) => Math.max(0, Math.min(1, band))),
            }));
          }
        })
        .catch(() => {
          if (!cancelled) {
            setState((current) =>
              current.recordingBands.length === 0 ? current : { ...current, recordingBands: [] },
            );
          }
        });
    };

    pollBands();
    const interval = window.setInterval(pollBands, recordingFftPollMs);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [state.recording]);

  useEffect(() => {
    if (state.recording === null) {
      return undefined;
    }

    let cancelled = false;
    lastRecordingLevelCommitRef.current = window.performance.now() - recordingLevelCommitMs;
    const commitLevel = (level: number) => {
      const normalized = Math.max(0, Math.min(1, level));
      const now = window.performance.now();
      const previous = recordingLevelRef.current;
      if (
        now - lastRecordingLevelCommitRef.current < recordingLevelCommitMs ||
        Math.abs(normalized - previous) < recordingLevelDelta
      ) {
        return;
      }

      recordingLevelRef.current = normalized;
      lastRecordingLevelCommitRef.current = now;
      setState((current) => ({ ...current, recordingLevel: normalized }));
    };

    const pollLevel = () => {
      getRecordingLevel()
        .then((level) => {
          if (!cancelled) {
            commitLevel(level);
          }
        })
        .catch(() => {
          if (!cancelled && recordingLevelRef.current !== 0) {
            recordingLevelRef.current = 0;
            lastRecordingLevelCommitRef.current = window.performance.now();
            setState((current) => ({ ...current, recordingLevel: 0 }));
          }
        });
    };

    pollLevel();
    const interval = window.setInterval(pollLevel, recordingLevelPollMs);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [state.recording]);

  return (
    <ErrorBoundary
      fallback={
        <div role="alert" className="error-boundary error-boundary--overlay">
          <p>Wind Speak encountered an error. Tap to reload.</p>
          <button
            type="button"
            onClick={() => {
              if (typeof window !== "undefined") {
                window.location.reload();
              }
            }}
          >
            Reload
          </button>
        </div>
      }
    >
    <main className="overlay-shell" data-tauri-drag-region>
      <RecorderOverlay
        recording={state.recording}
        elapsedSeconds={state.elapsedSeconds}
        busy={state.busy}
        phase={state.recorderPhase}
        modelStatus={state.modelStatus}
        hotkeyLabel={state.shortcutStatus?.hotkey || "BUTTON"}
        notice={state.notice.message}
        liveTranscript={state.liveTranscript}
        inputLevel={state.recordingLevel}
        inputBands={state.recordingBands}
        overlaySize={overlaySize}
        onMoveStart={handleMoveStart}
        onResizeStart={handleResizeStart}
        onCycleSize={handleCycleOverlaySize}
        onOpenHub={handleOpenHub}
        onToggle={() => void handleDictationAction("toggle")}
        onPressStart={() => void handleDictationAction("pressed")}
        onPressEnd={() => void handleDictationAction("released")}
        onCancel={() => void handleDictationAction("cancel")}
      />
    </main>
    </ErrorBoundary>
  );
}

function parseOptionalWordCount(value: string) {
  const trimmed = value.trim();
  if (trimmed.length === 0) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

function HistoryPanel({
  sessions,
  onCopy,
  onInject,
  onPolish,
  polishingSessionId,
  onExport,
  onUpdateNotes,
  recentApps,
}: {
  sessions: TranscriptSession[];
  onCopy: (session: TranscriptSession) => Promise<void>;
  onInject: (session: TranscriptSession) => Promise<void>;
  onPolish: (session: TranscriptSession) => Promise<void>;
  polishingSessionId: string | null;
  onExport: (session: TranscriptSession, format: ExportFormat) => Promise<void>;
  onUpdateNotes: (session: TranscriptSession, notes: string) => Promise<void>;
  recentApps: RecentAppUsage[];
}) {
  const [query, setQuery] = useState("");
  const [fromDate, setFromDate] = useState("");
  const [toDate, setToDate] = useState("");
  const [minWords, setMinWords] = useState("");
  const [maxWords, setMaxWords] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [draftNotes, setDraftNotes] = useState<Record<string, string>>({});
  const [filtered, setFiltered] = useState<TranscriptSession[]>(sessions);
  const [filterError, setFilterError] = useState<string | null>(null);

  const searchFilters = useMemo<HistorySearchFilters>(
    () => ({
      query: query.trim() || null,
      fromDate: fromDate || null,
      toDate: toDate || null,
      minWordCount: parseOptionalWordCount(minWords),
      maxWordCount: parseOptionalWordCount(maxWords),
      limit: 200,
    }),
    [fromDate, maxWords, minWords, query, toDate],
  );

  useEffect(() => {
    let cancelled = false;
    const runSearch = async () => {
      const hasFilters =
        searchFilters.query !== null ||
        searchFilters.fromDate !== null ||
        searchFilters.toDate !== null ||
        searchFilters.minWordCount !== null ||
        searchFilters.maxWordCount !== null;
      if (!hasFilters) {
        setFiltered(sessions);
        setFilterError(null);
        return;
      }

      try {
        const nextSessions = await searchSessions(searchFilters);
        if (!cancelled) {
          setFiltered(nextSessions);
          setFilterError(null);
        }
      } catch (error) {
        if (!cancelled) {
          setFiltered(sessions);
          setFilterError(stringifyError(error));
        }
      }
    };

    const timer = window.setTimeout(() => {
      void runSearch();
    }, 160);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [searchFilters, sessions]);

  return (
    <section className="list-panel">
      <PanelTitle icon={<History size={22} />} title="Transcript history" />
      <div className="history-toolbar">
        <input
          type="search"
          placeholder="Search transcripts, apps, notes"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <input
          type="date"
          aria-label="From date"
          value={fromDate}
          onChange={(event) => setFromDate(event.currentTarget.value)}
        />
        <input
          type="date"
          aria-label="To date"
          value={toDate}
          onChange={(event) => setToDate(event.currentTarget.value)}
        />
        <input
          type="number"
          min="0"
          inputMode="numeric"
          placeholder="Min words"
          value={minWords}
          onChange={(event) => setMinWords(event.currentTarget.value)}
        />
        <input
          type="number"
          min="0"
          inputMode="numeric"
          placeholder="Max words"
          value={maxWords}
          onChange={(event) => setMaxWords(event.currentTarget.value)}
        />
      </div>
      {filterError ? <p className="history-filter-error">Filter unavailable: {filterError}</p> : null}
      {recentApps.length > 0 ? (
        <div className="recent-apps">
          <span className="eyebrow">Top apps</span>
          <div className="recent-apps__row">
            {recentApps.map((app) => (
              <span className="chip" key={app.name} title={`${app.sessionCount} sessions`}>
                {app.name} <small>· {app.category}</small>
              </span>
            ))}
          </div>
        </div>
      ) : null}
      {filtered.length === 0 ? (
        <EmptyState
          text={
            sessions.length === 0
              ? "No transcripts yet. Start a recording from the floating control."
              : "No transcripts match the current search."
          }
        />
      ) : (
        filtered.map((session) => {
          const expanded = expandedId === session.id;
          const minutes = session.durationMs / 60_000;
          const wpm = minutes > 0 ? Math.round(session.wordCount / minutes) : 0;
          return (
            <article
              className={`history-item ${expanded ? "is-expanded" : ""}`}
              key={session.id}
            >
              <div className="history-item__row">
                <button
                  type="button"
                  className="history-item__summary"
                  onClick={() => setExpandedId(expanded ? null : session.id)}
                  aria-expanded={expanded}
                >
                  <span className="history-item__date">{formatDate(session.createdAt)}</span>
                  <p>{session.cleanedText}</p>
                  <small>
                    {session.wordCount} words · {formatDuration(session.durationMs)} · {wpm} wpm
                    {session.appName ? ` · ${session.appName}` : ""}
                  </small>
                </button>
                <div className="history-item__actions">
                  <button
                    className="button button--ghost button--square"
                    type="button"
                    onClick={() => {
                      void onCopy(session);
                    }}
                    aria-label="Copy transcript"
                    title="Copy transcript"
                  >
                    <Copy size={18} />
                  </button>
                  <button
                    className="button button--ghost button--square"
                    type="button"
                    onClick={() => {
                      void onInject(session);
                    }}
                    aria-label="Paste transcript again"
                    title="Paste transcript again"
                  >
                    <Clipboard size={18} />
                  </button>
                  <button
                    className="button button--ghost button--square"
                    type="button"
                    onClick={() => {
                      void onPolish(session);
                    }}
                    disabled={polishingSessionId === session.id}
                    aria-label="AI edit transcript"
                    title="AI edit transcript"
                  >
                    <Sparkles size={18} />
                  </button>
                </div>
              </div>
              {expanded ? (
                <div className="history-item__detail">
                  <div className="history-item__stats">
                    <span>
                      <strong>{session.wordCount}</strong> words
                    </span>
                    <span>
                      <strong>{formatDuration(session.durationMs)}</strong> duration
                    </span>
                    <span>
                      <strong>{wpm}</strong> wpm
                    </span>
                    <span>
                      <strong>{session.appName ?? "Unknown"}</strong> app
                    </span>
                  </div>
                  {session.audioPath ? (
                    <audio
                      controls
                      preload="none"
                      src={
                        session.audioPath.startsWith("http") ||
                        session.audioPath.startsWith("app:") ||
                        session.audioPath.startsWith("mock:")
                          ? session.audioPath
                          : `tauri://localhost/${session.audioPath.replace(/\\/g, "/")}`
                      }
                    />
                  ) : null}
                  <label>
                    <span>Notes</span>
                    <textarea
                      value={draftNotes[session.id] ?? session.notes}
                      rows={2}
                      onChange={(event) =>
                        setDraftNotes({ ...draftNotes, [session.id]: event.currentTarget.value })
                      }
                      onBlur={() => {
                        const value = draftNotes[session.id] ?? session.notes;
                        if (value !== session.notes) {
                          void onUpdateNotes(session, value);
                        }
                      }}
                    />
                  </label>
                  <div className="history-item__exports">
                    {(["txt", "md", "json", "srt"] as ExportFormat[]).map((format) => (
                      <button
                        key={format}
                        className="button button--ghost"
                        type="button"
                        onClick={() => void onExport(session, format)}
                      >
                        <Download size={14} /> {format.toUpperCase()}
                      </button>
                    ))}
                  </div>
                </div>
              ) : null}
            </article>
          );
        })
      )}
    </section>
  );
}

function Onboarding({
  settings,
  setSettings,
  microphones,
  modelStatus,
  shortcutStatus,
  shortcutTest,
  micCheck,
  onStartMicCheck,
  onStopMicCheck,
  onTestShortcut,
  pasteTest,
  onPasteTest,
  onShowFloatingControl,
  onComplete,
}: {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  modelStatus: ModelStatus | null;
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  micCheck: MicCheckState;
  onStartMicCheck: () => Promise<void>;
  onStopMicCheck: () => Promise<void>;
  onTestShortcut: () => void;
  pasteTest: PasteTestState;
  onPasteTest: () => Promise<void>;
  onShowFloatingControl: () => Promise<void>;
  onComplete: () => Promise<void>;
}) {
  const micStatusTone = micCheck.active
    ? "hot"
    : micCheck.passed
      ? "good"
      : microphones.length > 0
        ? "idle"
        : "warn";

  return (
    <main className="onboarding-shell">
      <section className="onboarding-panel">
        <div className="brand-block">
          <span className="brand-block__index">0001</span>
          <div>
            <p className="eyebrow">Wind Speak</p>
            <h1>Desktop dictation instrument</h1>
          </div>
        </div>
        <div className="onboarding-grid">
          <article className="onboarding-step">
            <StatusLed tone={modelStatus?.ready ? "good" : "warn"} label="Bundled runtime" />
            <h2>Install once. Speak anywhere.</h2>
            <p>
              The local whisper.cpp runtime and Base English model are packaged with the app.
              Advanced paths stay hidden unless you turn them on.
            </p>
          </article>
          <article className="onboarding-step">
            <StatusLed tone={micStatusTone} label="Microphone check" />
            <label>
              <span>Input device</span>
              <select
                value={settings.microphoneName ?? ""}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    microphoneName:
                      event.currentTarget.value.length > 0 ? event.currentTarget.value : null,
                  })
                }
              >
                <option value="">System default</option>
                {microphones.map((microphone) => (
                  <option key={microphone.name} value={microphone.name}>
                    {microphone.name}
                    {microphone.isDefault ? " (default)" : ""}
                  </option>
                ))}
              </select>
            </label>
            <div className="shortcut-test">
              <button
                className="button button--ghost"
                type="button"
                onClick={() => void (micCheck.active ? onStopMicCheck() : onStartMicCheck())}
                disabled={microphones.length === 0}
              >
                <Mic size={18} />
                {micCheck.active ? "Stop mic check" : "Start mic check"}
              </button>
              <p>{micCheck.message}</p>
            </div>
            <div
              className={clsx("mic-meter", "mic-meter--live", micCheck.active && "is-listening")}
              aria-hidden="true"
            >
              {Array.from({ length: 12 }, (_, index) => (
                <span
                  key={index}
                  style={{
                    transform: `scaleY(${meterBarScale(index, micCheck.level, micCheck.active)})`,
                  }}
                />
              ))}
            </div>
          </article>
          <article className="onboarding-step">
            <StatusLed
              tone={shortcutStatus?.registered ? "good" : "warn"}
              label={shortcutStatus?.hotkey || "Overlay fallback"}
            />
            <h2>Hold the shortcut, talk, release.</h2>
            <p>
              {shortcutStatus?.message ??
                "Wind Speak registers a global shortcut on launch and keeps the floating control available if a shortcut is taken."}
            </p>
            <div className="shortcut-test">
              <button className="button button--ghost" type="button" onClick={onTestShortcut}>
                <Keyboard size={18} />
                Test active shortcut
              </button>
              <p>{shortcutTest.message}</p>
            </div>
            <label>
              <span>Shortcut</span>
              <select
                value={settings.hotkey}
                onChange={(event) => setSettings({ ...settings, hotkey: event.currentTarget.value })}
              >
                {shortcutOptions.map((shortcut) => (
                  <option key={shortcut} value={shortcut}>
                    {shortcut}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>Capture mode</span>
              <select
                value={settings.mode}
                onChange={(event) =>
                  setSettings({ ...settings, mode: event.currentTarget.value as AppSettings["mode"] })
                }
              >
                <option value="pushToTalk">Push-to-talk</option>
                <option value="toggle">Toggle</option>
              </select>
            </label>
          </article>
          <article className="onboarding-step">
            <StatusLed tone="good" label="Floating control" />
            <h2>Recorder pill stays above your apps.</h2>
            <p>
              Wind Speak opens the always-on-top control during onboarding. Use this recovery action
              if Windows moved or hid it.
            </p>
            <div className="shortcut-test">
              <button
                className="button button--ghost"
                type="button"
                onClick={() => void onShowFloatingControl()}
              >
                <Radio size={18} />
                Show floating control
              </button>
              <p>Use the pill, tray, or shortcut to start and stop dictation.</p>
            </div>
          </article>
          <article className="onboarding-step onboarding-step--accent">
            <StatusLed tone={pasteTest.passed ? "good" : "idle"} label="Private by default" />
            <h2>First paste test</h2>
            <p>
              Focus Notepad or any text field, then run the same native paste path Wind Speak uses
              after transcription.
            </p>
            <div className="shortcut-test">
              <button
                className="button button--ghost"
                type="button"
                onClick={() => void onPasteTest()}
                disabled={pasteTest.running}
              >
                <Clipboard size={18} />
                {pasteTest.running ? "Testing paste" : "Test paste"}
              </button>
              <p>{pasteTest.message}</p>
            </div>
            <button
              className="button button--primary"
              type="button"
              onClick={() => void onComplete()}
              disabled={!modelStatus?.ready || micCheck.active}
            >
              <CheckCircle2 size={18} />
              Enter hub
            </button>
          </article>
        </div>
      </section>
    </main>
  );
}

function meterBarScale(index: number, level: number, active: boolean) {
  if (!active && level === 0) {
    return 0.16;
  }

  const center = 5.5;
  const distanceFromCenter = Math.abs(index - center);
  const contour = 1 - distanceFromCenter / 7;
  const signal = Math.max(level, active ? 0.08 : 0);
  return Math.max(0.12, Math.min(1, 0.14 + signal * (0.45 + contour)));
}

function AdvancedPanel({
  settings,
  setSettings,
  modelStatus,
  modelInventory,
  onSave,
}: {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  modelStatus: ModelStatus | null;
  modelInventory: ModelInventory | null;
  onSave: () => Promise<void>;
}) {
  return (
    <section className="settings-panel">
      <PanelTitle icon={<Cpu size={22} />} title="Advanced runtime" />
      <StatusLed tone={modelStatus?.ready ? "good" : "warn"} label={modelStatus?.message ?? "Checking"} />
      <div className="instruction-card">
        <h3>Bundled by default</h3>
        <p>
          Wind Speak ships with whisper.cpp and Base English. Override these paths only when
          testing a custom build or a larger local model.
        </p>
      </div>
      <label>
        <span>Custom instructions</span>
        <textarea
          value={settings.customInstructions}
          onChange={(event) =>
            setSettings({ ...settings, customInstructions: event.currentTarget.value })
          }
          placeholder="e.g. Always expand acronyms; never insert emojis; rewrite as bullet points."
          rows={3}
        />
      </label>
      <div className="model-grid">
        {modelInventory?.models.map((model) => {
          const isActive = modelInventory?.activeModelId === model.id && !settings.advancedRuntimeEnabled;
          return (
            <button
              type="button"
              key={model.id}
              className={`model-pill ${isActive ? "model-pill--active" : ""}`}
              onClick={() => {
                if (model.installed) {
                  setSettings({ ...settings, activeModelId: model.id });
                }
              }}
              disabled={!model.installed}
            >
              <strong>{model.label}</strong>
              <span>
                {!model.installed
                  ? "Not installed"
                  : isActive
                    ? "Active"
                    : model.bundled
                      ? "Bundled"
                      : "Installed"}
              </span>
            </button>
          );
        })}
      </div>
      <label>
        <span>Active model</span>
        <select
          value={settings.activeModelId}
          onChange={(event) =>
            setSettings({ ...settings, activeModelId: event.currentTarget.value })
          }
          disabled={settings.advancedRuntimeEnabled}
        >
          {modelInventory?.models
            .filter((model) => model.installed)
            .map((model) => (
              <option key={model.id} value={model.id}>
                {model.label}
              </option>
            ))}
        </select>
      </label>
      <ToggleRow
        icon={<Cpu size={18} />}
        label="Use advanced runtime override"
        checked={settings.advancedRuntimeEnabled}
        onChange={(advancedRuntimeEnabled) => setSettings({ ...settings, advancedRuntimeEnabled })}
      />
      <label>
        <span>whisper-cli.exe</span>
        <input
          value={settings.advancedWhisperCliPath}
          onChange={(event) =>
            setSettings({ ...settings, advancedWhisperCliPath: event.currentTarget.value })
          }
          disabled={!settings.advancedRuntimeEnabled}
          placeholder="C:\tools\whisper.cpp\build\bin\Release\whisper-cli.exe"
        />
      </label>
      <label>
        <span>ggml-base.en.bin</span>
        <input
          value={settings.advancedModelPath}
          onChange={(event) => setSettings({ ...settings, advancedModelPath: event.currentTarget.value })}
          disabled={!settings.advancedRuntimeEnabled}
          placeholder="C:\models\ggml-base.en.bin"
        />
      </label>
      <button className="button button--primary" type="button" onClick={() => void onSave()}>
        <CheckCircle2 size={18} />
        Save runtime settings
      </button>
    </section>
  );
}

function SettingsPanel({
  settings,
  setSettings,
  microphones,
  shortcutStatus,
  shortcutTest,
  runtimeEvents,
  onTestShortcut,
  onToggleShortcutsPaused,
  onShowFloatingControl,
  onRerunOnboarding,
  onSave,
  updateStatus,
  updateResult,
  onCheckUpdates,
  onInstallUpdate,
  feedbackDraft,
  setFeedbackDraft,
  onSubmitFeedback,
}: {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  runtimeEvents: RuntimeEvent[];
  onTestShortcut: () => void;
  onToggleShortcutsPaused: () => Promise<void>;
  onShowFloatingControl: () => Promise<void>;
  onRerunOnboarding: () => Promise<void>;
  onSave: () => Promise<void>;
  updateStatus: UpdateStatus;
  updateResult: UpdateCheckResult | null;
  onCheckUpdates: () => Promise<void>;
  onInstallUpdate: () => Promise<void>;
  feedbackDraft: string;
  setFeedbackDraft: (value: string) => void;
  onSubmitFeedback: () => Promise<void>;
}) {
  return (
    <section className="settings-panel">
      <PanelTitle icon={<Keyboard size={22} />} title="Input and privacy" />
      <label>
        <span>Microphone</span>
        <select
          value={settings.microphoneName ?? ""}
          onChange={(event) =>
            setSettings({
              ...settings,
              microphoneName: event.currentTarget.value.length > 0 ? event.currentTarget.value : null,
            })
          }
        >
          <option value="">System default</option>
          {microphones.map((microphone) => (
            <option key={microphone.name} value={microphone.name}>
              {microphone.name}
              {microphone.isDefault ? " (default)" : ""}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>Recognition language</span>
        <select
          value={settings.language ?? "auto"}
          onChange={(event) =>
            setSettings({
              ...settings,
              language:
                event.currentTarget.value === "auto" ? null : event.currentTarget.value,
            })
          }
        >
          {languageOptions.map((language) => (
            <option key={language.value} value={language.value}>
              {language.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>Shortcut</span>
        <select
          value={settings.hotkey}
          onChange={(event) => setSettings({ ...settings, hotkey: event.currentTarget.value })}
        >
          {shortcutOptions.map((shortcut) => (
            <option key={shortcut} value={shortcut}>
              {shortcut}
            </option>
          ))}
        </select>
      </label>
      <div className="instruction-card">
        <h3>Global shortcut</h3>
        <p>
          {shortcutStatus?.message ??
            "Wind Speak registers your saved shortcut when the desktop app starts."}
        </p>
        <div className="shortcut-test">
          <button
            className="button button--ghost"
            type="button"
            onClick={() => void onToggleShortcutsPaused()}
          >
            <Zap size={18} />
            {shortcutStatus?.paused ? "Resume shortcuts" : "Pause shortcuts"}
          </button>
          <button className="button button--ghost" type="button" onClick={onTestShortcut}>
            <Keyboard size={18} />
            Test active shortcut
          </button>
          <p>{shortcutTest.message}</p>
        </div>
        <RuntimeEventList events={runtimeEvents} />
      </div>
      <div className="instruction-card">
        <h3>Send feedback</h3>
        <p>
          Sends your message with recent runtime events and sanitized settings. Local paths,
          runtime override paths, and webhook URL are not included in the report.
        </p>
        <label>
          <span>Feedback webhook URL</span>
          <input
            value={settings.feedbackWebhookUrl}
            onChange={(event) =>
              setSettings({ ...settings, feedbackWebhookUrl: event.currentTarget.value })
            }
            placeholder="https://example.com/wind-speak-feedback"
          />
        </label>
        <label>
          <span>Feedback message</span>
          <textarea
            value={feedbackDraft}
            onChange={(event) => setFeedbackDraft(event.currentTarget.value)}
            placeholder="What happened? What were you trying to do?"
            rows={3}
          />
        </label>
        <button
          className="button button--ghost"
          type="button"
          onClick={() => void onSubmitFeedback()}
          disabled={feedbackDraft.trim().length === 0}
        >
          <Database size={18} />
          Send feedback
        </button>
      </div>
      <div className="instruction-card update-card">
        <div>
          <p className="eyebrow">Always-on-top recorder</p>
          <h3>Floating control</h3>
          <p>Show and reset the desktop recorder pill if it was hidden or moved off screen.</p>
        </div>
        <button
          className="button button--ghost"
          type="button"
          onClick={() => void onShowFloatingControl()}
        >
          <Radio size={18} />
          Show floating control
        </button>
      </div>
      <label>
        <span>Mode</span>
        <select
          value={settings.mode}
          onChange={(event) =>
            setSettings({ ...settings, mode: event.currentTarget.value as AppSettings["mode"] })
          }
        >
          <option value="toggle">Toggle</option>
          <option value="pushToTalk">Push-to-talk</option>
        </select>
      </label>
      <ToggleRow
        icon={<Clipboard size={18} />}
        label="Restore clipboard after paste"
        checked={settings.restoreClipboard}
        onChange={(restoreClipboard) => setSettings({ ...settings, restoreClipboard })}
      />
      <ToggleRow
        icon={<Zap size={18} />}
        label="Auto-inject after transcription"
        checked={settings.autoInject}
        onChange={(autoInject) => setSettings({ ...settings, autoInject })}
      />
      <label>
        <span>Injection mode</span>
        <select
          value={settings.injectionMode}
          onChange={(event) =>
            setSettings({
              ...settings,
              injectionMode: event.currentTarget.value as AppSettings["injectionMode"],
            })
          }
          disabled={!settings.autoInject}
        >
          <option value="auto">Auto-paste into focused app</option>
          <option value="clipboard">Copy to clipboard only</option>
        </select>
      </label>
      <ToggleRow
        icon={<RotateCw size={18} />}
        label="Cleanup punctuation, corrections, and dictionary terms"
        checked={settings.cleanupEnabled}
        onChange={(cleanupEnabled) => setSettings({ ...settings, cleanupEnabled })}
      />
      <div className="instruction-card">
        <h3>AI edit</h3>
        <ToggleRow
          icon={<Sparkles size={18} />}
          label="Auto-edit before paste"
          checked={settings.autoPolish}
          onChange={(autoPolish) => setSettings({ ...settings, autoPolish })}
        />
        <label>
          <span>Style</span>
          <select
            value={settings.polishStyle}
            onChange={(event) =>
              setSettings({
                ...settings,
                polishStyle: event.currentTarget.value as AppSettings["polishStyle"],
              })
            }
          >
            {polishStyleOptions.map((style) => (
              <option key={style.value} value={style.value}>
                {style.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Provider</span>
          <select
            value={settings.polishProvider}
            onChange={(event) =>
              setSettings({
                ...settings,
                polishProvider: event.currentTarget.value as AppSettings["polishProvider"],
              })
            }
          >
            <option value="ollama">Local Ollama</option>
            <option value="openAiCompatible">OpenAI-compatible</option>
            <option value="disabled">Disabled</option>
          </select>
        </label>
        <label>
          <span>Endpoint</span>
          <input
            value={settings.polishEndpoint}
            onChange={(event) =>
              setSettings({ ...settings, polishEndpoint: event.currentTarget.value })
            }
            placeholder={
              settings.polishProvider === "openAiCompatible"
                ? "https://api.openai.com/v1/chat/completions"
                : "http://127.0.0.1:11434/api/chat"
            }
          />
        </label>
        <label>
          <span>Model</span>
          <input
            value={settings.polishModel}
            onChange={(event) =>
              setSettings({ ...settings, polishModel: event.currentTarget.value })
            }
            placeholder={settings.polishProvider === "openAiCompatible" ? "gpt-4o-mini" : "llama3.2"}
          />
        </label>
        {settings.polishProvider === "openAiCompatible" ? (
          <small>Uses WIND_SPEAK_POLISH_API_KEY from the desktop environment.</small>
        ) : null}
      </div>
      <ToggleRow
        icon={<Radio size={18} />}
        label="Show live words in the floating control"
        checked={settings.livePreviewEnabled}
        onChange={(livePreviewEnabled) => setSettings({ ...settings, livePreviewEnabled })}
      />
      <label>
        <span>Live preview cadence</span>
        <select
          value={settings.livePreviewIntervalMs}
          onChange={(event) =>
            setSettings({
              ...settings,
              livePreviewIntervalMs: Number(event.currentTarget.value),
            })
          }
          disabled={!settings.livePreviewEnabled}
        >
          <option value={500}>Fast - 500 ms</option>
          <option value={750}>Balanced - 750 ms</option>
          <option value={1000}>Calm - 1000 ms</option>
          <option value={1500}>Light CPU - 1500 ms</option>
        </select>
      </label>
      <ToggleRow
        icon={<CheckCircle2 size={18} />}
        label="Run final accuracy pass before paste"
        checked={settings.finalPassEnabled}
        onChange={(finalPassEnabled) => setSettings({ ...settings, finalPassEnabled })}
      />
      <label>
        <span>Floating bubble size</span>
        <select
          value={settings.bubbleSize}
          onChange={(event) =>
            setSettings({
              ...settings,
              bubbleSize: event.currentTarget.value as AppSettings["bubbleSize"],
            })
          }
        >
          <option value="small">Small</option>
          <option value="medium">Medium</option>
          <option value="large">Large</option>
        </select>
      </label>
      <label>
        <span>Floating bubble opacity ({Math.round(settings.bubbleOpacity * 100)}%)</span>
        <input
          type="range"
          min={0.2}
          max={1}
          step={0.05}
          value={settings.bubbleOpacity}
          onChange={(event) =>
            setSettings({ ...settings, bubbleOpacity: Number(event.currentTarget.value) })
          }
        />
      </label>
      <ToggleRow
        icon={<Zap size={18} />}
        label="Start with Windows"
        checked={settings.startAtLogin}
        onChange={(startAtLogin) => setSettings({ ...settings, startAtLogin })}
      />
      <div className="instruction-card update-card">
        <div>
          <p className="eyebrow">Signed update feed</p>
          <h3>App updates</h3>
          <p>
            {updateResult?.message ??
              "Wind Speak checks GitHub Releases for signed Tauri update metadata."}
          </p>
          {updateResult?.version && (
            <small>
              Current {updateResult.currentVersion} / available {updateResult.version}
            </small>
          )}
        </div>
        <div className="update-card__actions">
          <button
            className="button button--ghost"
            type="button"
            onClick={() => void onCheckUpdates()}
            disabled={updateStatus === "checking" || updateStatus === "downloading"}
          >
            <RotateCw size={18} />
            {updateStatus === "checking" ? "Checking" : "Check"}
          </button>
          <button
            className="button button--primary"
            type="button"
            onClick={() => void onInstallUpdate()}
            disabled={!updateResult?.available || updateStatus === "downloading"}
          >
            <Download size={18} />
            {updateStatus === "downloading" ? "Installing" : "Install"}
          </button>
        </div>
      </div>
      <div className="instruction-card update-card">
        <div>
          <p className="eyebrow">First-run checklist</p>
          <h3>Onboarding</h3>
          <p>Run the microphone, shortcut, and paste checks again without clearing history.</p>
        </div>
        <button className="button button--ghost" type="button" onClick={() => void onRerunOnboarding()}>
          <RotateCw size={18} />
          Run onboarding
        </button>
      </div>
      <button className="button button--primary" type="button" onClick={() => void onSave()}>
        <CheckCircle2 size={18} />
        Save settings
      </button>
    </section>
  );
}

function RuntimeEventList({ events }: { events: RuntimeEvent[] }) {
  const recent = events.slice(0, 6);
  return (
    <div className="runtime-events" aria-label="Runtime event log">
      <p className="eyebrow">Runtime signal</p>
      {recent.length === 0 ? (
        <p>No shortcut events recorded in this session.</p>
      ) : (
        <ul>
          {recent.map((event) => (
            <li key={`${event.createdAt}-${event.kind}-${event.message}`}>
              <span>{event.kind}</span>
              <p>{event.message}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function formatDuration(durationMs: number) {
  const seconds = Math.round(durationMs / 1000);
  return `${seconds}s`;
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function stringifyError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export default App;
