import { emit, listen } from "@tauri-apps/api/event";
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
  HubTab,
  MicrophoneInfo,
  RecentAppUsage,
  ModelInventory,
  ModelStatus,
  NativeDictationEvent,
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
    message: "Atmospeak is standing by.",
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
      bubbleSize: settingsDraft?.bubbleSize ?? "medium",
      bubbleOpacity: settingsDraft?.bubbleOpacity ?? 1,
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
    settingsDraft?.bubbleOpacity,
    settingsDraft?.bubbleSize,
    shortcutStatus,
  ]);


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
    setNotice({ tone: "neutral", message: "Focus the target app. Atmospeak will paste in 3 seconds." });
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
      const result = await injectText("Atmospeak paste test");
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
        <div className="boot__mark"><Aura size={48} active /></div>
        <p>Starting Atmospeak…</p>
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
        modelInventory={modelInventory}
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
          setNotice({ tone: "success", message: "Onboarding complete. Atmospeak is armed." });
        }}
      />
    );
  }

  return (
    <ErrorBoundary>
    <main className="hub-shell">
      <nav className="hub__nav" aria-label="Atmospeak sections">
        <div className="hub__brand">
          <span className="brand-aura"><Aura size={30} /></span>
          <span className="wm">
            <strong>Atmospeak</strong>
            <span>Local · on device</span>
          </span>
        </div>
        {tabs.map((tab) => {
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              className={clsx("hub__navitem", activeTab === tab.id && "active")}
              type="button"
              onClick={() => setActiveTab(tab.id)}
            >
              <Icon size={17} />
              <span className="lab">{tab.label}</span>
            </button>
          );
        })}
        <div className="hub__status" aria-label="Application status">
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
        <div className="marquee-foot">+ ON DEVICE + NO CLOUD + WHISPER · BASE.EN +</div>
      </nav>

      <div className="hub__main">
        <section className="notice-rail" aria-live="polite">
          <span className={clsx("notice-rail__tone", `notice-rail__tone--${notice.tone}`)} />
          <p>{notice.message}</p>
        </section>

        <div className="hub__panel">
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
      </div>
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
  bubbleSize: AppSettings["bubbleSize"];
  bubbleOpacity: number;
}

function OverlayWindow() {
  const [state, setState] = useState<OverlayStatePayload>({
    recording: null,
    elapsedSeconds: 0,
    busy: false,
    recorderPhase: "idle",
    modelStatus: null,
    shortcutStatus: null,
    notice: { tone: "neutral", message: "Atmospeak is standing by." },
    recordingLevel: 0,
    recordingBands: [],
    liveTranscript: emptyLiveTranscript,
    bubbleSize: "medium",
    bubbleOpacity: 1,
  });
  const recordingLevelRef = useRef(0);
  const lastRecordingLevelCommitRef = useRef(0);

  useEffect(() => {
    document.body.classList.add("is-overlay-window");
    return () => document.body.classList.remove("is-overlay-window");
  }, []);

  const handleMoveStart = useCallback(() => {
    if (!hasTauriRuntime()) {
      return;
    }
    getCurrentWindow().startDragging().catch(() => undefined);
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
          <p>Atmospeak encountered an error. Tap to reload.</p>
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
    <main className="overlay-shell">
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
        bubbleSize={state.bubbleSize}
        bubbleOpacity={state.bubbleOpacity}
        onMoveStart={handleMoveStart}
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

function stringifyError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export default App;
