import { listen } from "@tauri-apps/api/event";
import { PhysicalPosition, getCurrentWindow } from "@tauri-apps/api/window";
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
import { flushSync } from "react-dom";
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
  cancelShortcutCapture,
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
  getPolishInventory,
  getRuntimeEvents,
  getShortcutStatus,
  handleDictationAction,
  hasTauriRuntime,
  polishSession,
  setSessionPreferPolished,
  ensurePolishRuntime,
  injectOnboardingSample,
  injectSession,
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
  ModelInventoryItem,
  ModelDownloadProgress,
  ModelStatus,
  LiveTranscriptEvent,
  NativeDictationEvent,
  RecordingStarted,
  RuntimeEvent,
  ShortcutCaptureEvent,
  ShortcutKeyEvent,
  ShortcutStatus,
  Snippet,
  SoundCheckResult,
  StageMetrics,
  UpdateCheckResult,
  UpdateStatus,
} from "./types/dictation";
import { ONBOARDING_VERSION, sessionDisplayText } from "./types/dictation";

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

interface ShortcutCaptureState {
  arming: boolean;
  active: boolean;
  keys: string[];
  message: string;
}

const SHORTCUT_MODIFIERS = ["Ctrl", "Win", "Alt", "Shift"] as const;

function sortShortcutKeys(keys: Iterable<string>): string[] {
  const unique = [...new Set(keys)];
  return [
    ...SHORTCUT_MODIFIERS.filter((modifier) => unique.includes(modifier)),
    ...unique.filter(
      (key) => !SHORTCUT_MODIFIERS.includes(key as (typeof SHORTCUT_MODIFIERS)[number]),
    ),
  ];
}

function focusedKeyLabel(event: KeyboardEvent): string | null {
  if (event.key === "Control") return "Ctrl";
  if (event.key === "Meta") return "Win";
  if (event.key === "Alt") return "Alt";
  if (event.key === "Shift") return "Shift";
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.code)) return event.code;
  if (/^Numpad[0-9]$/.test(event.code)) return event.code;
  const labels: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Escape: "Escape",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    ArrowUp: "Up",
    ArrowDown: "Down",
    CapsLock: "CapsLock",
    PrintScreen: "PrintScreen",
    ScrollLock: "ScrollLock",
    Pause: "Pause",
    Semicolon: "Semicolon",
    Equal: "Equals",
    Comma: "Comma",
    Minus: "Minus",
    Period: "Period",
    Slash: "Slash",
    Backquote: "Backquote",
    BracketLeft: "BracketLeft",
    Backslash: "Backslash",
    BracketRight: "BracketRight",
    Quote: "Quote",
    NumpadAdd: "NumpadAdd",
    NumpadSubtract: "NumpadSubtract",
    NumpadMultiply: "NumpadMultiply",
    NumpadDivide: "NumpadDivide",
    NumpadDecimal: "NumpadDecimal",
  };
  return labels[event.code] ?? null;
}

function capturedShortcutLabel(keys: Iterable<string>): { label: string | null; error: string | null } {
  const ordered = sortShortcutKeys(keys);
  const modifierCount = ordered.filter((key) =>
    SHORTCUT_MODIFIERS.includes(key as (typeof SHORTCUT_MODIFIERS)[number]),
  ).length;
  const normalKeys = ordered.length - modifierCount;
  if (modifierCount === 0) {
    return { label: null, error: "Include Ctrl, Win, Alt, or Shift in the shortcut." };
  }
  if (normalKeys > 1) {
    return { label: null, error: "Use one main key with any modifiers, then try again." };
  }
  if (normalKeys === 0 && modifierCount < 2) {
    return { label: null, error: "A modifier-only shortcut needs at least two keys." };
  }
  return { label: ordered.join("+"), error: null };
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
  const [polishInventory, setPolishInventory] = useState<ModelInventoryItem[]>([]);
  const [polishSetupBusy, setPolishSetupBusy] = useState(false);
  const [polishSetupMessage, setPolishSetupMessage] = useState("");
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
  const [shortcutCapture, setShortcutCapture] = useState<ShortcutCaptureState>({
    arming: false,
    active: false,
    keys: [],
    message: "",
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
  const shortcutCaptureRef = useRef(shortcutCapture);
  const soundCheckStartRef = useRef<Promise<boolean> | null>(null);
  const soundCheckFinishInFlightRef = useRef(false);
  const shortcutPressedCodesRef = useRef(new Map<number, string>());
  const focusedPressedKeysRef = useRef(new Map<string, string>());
  const focusedCapturedKeysRef = useRef(new Set<string>());

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
    shortcutCaptureRef.current = shortcutCapture;
  }, [shortcutCapture]);
  useEffect(() => {
    shortcutStatusRef.current = shortcutStatus;
  }, [shortcutStatus]);

  const setBusyState = useCallback((nextBusy: boolean) => {
    busyRef.current = nextBusy;
  }, []);

  const completeRecordedShortcut = useCallback((label: string) => {
    void setShortcutTestActive(false);
    const setupAlreadyComplete = settingsRef.current?.onboardingComplete === true;
    const message = setupAlreadyComplete
      ? `${label} recorded. Save settings to activate it.`
      : `${label} recorded. Test the same chord once to continue.`;
    shortcutCaptureRef.current = {
      arming: false,
      active: false,
      keys: [],
      message,
    };
    flushSync(() => {
      setShortcutCapture(shortcutCaptureRef.current);
      setShortcutTest({
        active: false,
        detected: false,
        message,
      });
      setSettingsDraft((current) => {
        if (!current) return current;
        const next = { ...current, hotkey: label };
        settingsRef.current = next;
        return next;
      });
    });
  }, []);

  useEffect(() => {
    const updateFocusedKeys = () => {
      shortcutCaptureRef.current = {
        ...shortcutCaptureRef.current,
        keys: sortShortcutKeys(new Set(focusedPressedKeysRef.current.values())),
        message: shortcutCaptureRef.current.active
          ? focusedPressedKeysRef.current.size
            ? "Keep holding the chord, then release all keys to save it."
            : "Hold the keys you want together."
          : shortcutCaptureRef.current.message,
      };
      flushSync(() => setShortcutCapture(shortcutCaptureRef.current));
    };

    const onKeyDown = (event: KeyboardEvent) => {
      const recording = shortcutCaptureRef.current.active;
      const testing = shortcutTestRef.current.active;
      if (!recording && !testing) return;
      if (recording) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
      const key = focusedKeyLabel(event);
      if (!key) {
        if (recording) {
          shortcutCaptureRef.current = {
            ...shortcutCaptureRef.current,
            message: "That key is not supported. Try another chord.",
          };
          flushSync(() => setShortcutCapture(shortcutCaptureRef.current));
        }
        return;
      }
      if (event.repeat || focusedPressedKeysRef.current.has(event.code)) return;
      focusedPressedKeysRef.current.set(event.code, key);
      focusedCapturedKeysRef.current.add(key);
      updateFocusedKeys();
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const recording = shortcutCaptureRef.current.active;
      const testing = shortcutTestRef.current.active;
      if (!recording && !testing) return;
      if (recording) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
      focusedPressedKeysRef.current.delete(event.code);
      updateFocusedKeys();
      if (!recording) {
        if (focusedPressedKeysRef.current.size !== 0 || focusedCapturedKeysRef.current.size === 0) {
          return;
        }
        const captured = capturedShortcutLabel(focusedCapturedKeysRef.current);
        focusedCapturedKeysRef.current.clear();
        const expected = settingsRef.current?.hotkey;
        if (captured.label && captured.label === expected) {
          if (shortcutTestTimerRef.current !== null) {
            window.clearTimeout(shortcutTestTimerRef.current);
            shortcutTestTimerRef.current = null;
          }
          void setShortcutTestActive(false);
          shortcutTestRef.current = {
            active: false,
            detected: true,
            message: `${captured.label} matched. It will activate when setup is complete.`,
          };
          flushSync(() => setShortcutTest(shortcutTestRef.current));
          setNotice({ tone: "success", message: `${captured.label} detected.` });
        } else {
          const message = captured.label
            ? `You pressed ${captured.label}. Press ${expected ?? "the selected shortcut"} instead.`
            : captured.error ?? "That chord cannot be tested. Try another.";
          shortcutTestRef.current = {
            ...shortcutTestRef.current,
            message,
          };
          flushSync(() => setShortcutTest(shortcutTestRef.current));
        }
        return;
      }
      if (focusedPressedKeysRef.current.size !== 0 || focusedCapturedKeysRef.current.size === 0) {
        return;
      }
      const captured = capturedShortcutLabel(focusedCapturedKeysRef.current);
      focusedCapturedKeysRef.current.clear();
      if (captured.label) {
        completeRecordedShortcut(captured.label);
      } else {
        shortcutCaptureRef.current = {
          ...shortcutCaptureRef.current,
          keys: [],
          message: captured.error ?? "That chord cannot be used. Try another.",
        };
        flushSync(() => setShortcutCapture(shortcutCaptureRef.current));
      }
    };

    const onBlur = () => {
      if (!shortcutCaptureRef.current.active && !shortcutTestRef.current.active) return;
      void setShortcutTestActive(false);
      focusedPressedKeysRef.current.clear();
      focusedCapturedKeysRef.current.clear();
      if (shortcutTestRef.current.active) {
        shortcutCaptureRef.current = {
          ...shortcutCaptureRef.current,
          keys: [],
        };
        flushSync(() => setShortcutCapture(shortcutCaptureRef.current));
        return;
      }
      shortcutCaptureRef.current = {
        arming: false,
        active: false,
        keys: [],
        message: "Recording cancelled when Atmospeak lost focus. Click Record shortcut and retry.",
      };
      flushSync(() => setShortcutCapture(shortcutCaptureRef.current));
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [completeRecordedShortcut]);

  const refreshSnapshotOnly = useCallback(async () => {
    const next = await getAppSnapshot();
    setSnapshot(next);
    setSettingsDraft(next.settings);
    return next;
  }, []);

  const refresh = useCallback(async () => {
    const [next, mics, status, inventory, polishModels, shortcut, events, metrics] =
      await Promise.all([
        getAppSnapshot(),
        listMicrophones(),
        getModelStatus(),
        getModelInventory(),
        getPolishInventory(),
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
    setPolishInventory(polishModels);
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
      setBusyState(payload.phase === "finalizing" || payload.phase === "listening");
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
          "atmospeak://shortcut-capture",
          (payload) => {
            const captureEvent = payload as ShortcutCaptureEvent;
            if (!shortcutCaptureRef.current.active) return;

            if (captureEvent.completed) {
              const label = captureEvent.completed;
              completeRecordedShortcut(label);
              void cancelShortcutCapture().catch((error: unknown) => {
                setShortcutCapture((current) => ({
                  ...current,
                  message: `Recorded ${label}, but cleanup failed: ${stringifyError(error)}`,
                }));
              });
              return;
            }

            shortcutCaptureRef.current = {
              ...shortcutCaptureRef.current,
              keys: captureEvent.keys,
              message:
                captureEvent.error ??
                (captureEvent.keys.length
                  ? "Keep holding the chord, then release all keys to save it."
                  : "Hold the keys you want together."),
            };
            setShortcutCapture(shortcutCaptureRef.current);
          },
        ],
        [
          "atmospeak://shortcut-key",
          (payload) => {
            if (
              !shortcutCaptureRef.current.active &&
              !shortcutTestRef.current.active
            ) {
              return;
            }
            const keyEvent = payload as ShortcutKeyEvent;
            const pressedCodes = shortcutPressedCodesRef.current;
            if (keyEvent.pressed) {
              pressedCodes.set(keyEvent.code, keyEvent.key);
            } else {
              pressedCodes.delete(keyEvent.code);
            }

            const pressedKeys = new Set(pressedCodes.values());
            shortcutCaptureRef.current = {
              ...shortcutCaptureRef.current,
              keys: sortShortcutKeys(pressedKeys),
            };
            setShortcutCapture(shortcutCaptureRef.current);
          },
        ],
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
      void setShortcutTestActive(false);
      void cancelShortcutCapture();
    };
  }, [applyNativeDictation, completeRecordedShortcut]);

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

  const startPhraseCheck = useCallback(() => {
    if (soundCheckStartRef.current || soundCheckFinishInFlightRef.current) {
      return Promise.resolve();
    }
    const operation = (async () => {
      const currentSettings = settingsRef.current;
      const deviceName = currentSettings?.microphoneName;
      if (!deviceName) {
        setSoundCheck({ active: false, result: null, message: "Choose a microphone first." });
        return false;
      }
      setSoundCheck({ active: true, result: null, message: "Starting microphone..." });
      try {
        await startSoundCheck(deviceName);
        setSoundCheck({ active: true, result: null, message: "Listening..." });
        return true;
      } catch (error: unknown) {
        setSoundCheck({ active: false, result: null, message: stringifyError(error) });
        return false;
      }
    })();
    soundCheckStartRef.current = operation;
    return operation.then(() => undefined);
  }, []);

  const finishPhraseCheck = useCallback(async () => {
    if (soundCheckFinishInFlightRef.current) return;
    soundCheckFinishInFlightRef.current = true;
    const startOperation = soundCheckStartRef.current;
    soundCheckStartRef.current = null;
    const started = startOperation ? await startOperation : false;
    if (!started) {
      soundCheckFinishInFlightRef.current = false;
      return;
    }
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
    } finally {
      soundCheckFinishInFlightRef.current = false;
    }
  }, [refreshSnapshotOnly]);

  const onSaveSettings = useCallback(async () => {
    if (!settingsDraft) return;
    setBusyState(true);
    try {
      let confirmKeyRebinding = false;
      const bound = settingsDraft.polishApiKeyOrigin?.trim() ?? "";
      if (bound && settingsDraft.polishProvider === "openaiCompatible") {
        try {
          const endpoint = settingsDraft.polishEndpoint.trim();
          const origin = new URL(endpoint).origin;
          if (origin && bound.toLowerCase() !== origin.toLowerCase()) {
            confirmKeyRebinding = window.confirm(
              `Send the saved polish API key to ${origin}?\n\nIt is currently bound to ${bound}.`,
            );
            if (!confirmKeyRebinding) {
              setNotice({
                tone: "error",
                message: "Settings not saved. Clear the API key or confirm rebinding.",
              });
              return;
            }
          }
        } catch {
          // Native validation will surface a clearer error.
        }
      }
      const next = await saveSettings(settingsDraft, { confirmKeyRebinding });
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
      const installedModel = inventory.models.find((model) => model.id === modelId);
      const installedBytes =
        installedModel?.sizeMb != null
          ? Math.round(installedModel.sizeMb * 1024 * 1024)
          : 0;
      setModelDownload({
        modelId,
        status: "installed",
        bytesDownloaded: installedBytes,
        totalBytes: installedBytes || null,
        percent: 100,
        message: `${installedModel?.label ?? modelId} installed and verified.`,
      });
      const current = settingsRef.current;
      if (current && !current.onboardingComplete) {
        const selected = {
          ...current,
          activeModelId: modelId,
          onboardingComplete: false,
          onboardingVersion: "",
          audioCalibration: null,
        };
        const saved = await saveSettings(selected);
        setSnapshot(saved);
        setSettingsDraft(saved.settings);
      }
      setModelStatus(await getModelStatus());
      setNotice({
        tone: "success",
        message: current?.onboardingComplete
          ? `${modelId} is installed. Choose Use, then save Settings to activate it.`
          : `${modelId} is installed and ready for setup.`,
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
        shortcutCapture={shortcutCapture}
        micCheck={micCheck}
        soundCheck={soundCheck}
        onStartMicCheck={startMicCheck}
        onStopMicCheck={stopMicCheck}
        onStartSoundCheck={startPhraseCheck}
        onFinishSoundCheck={finishPhraseCheck}
        onOpenWindowsSoundSettings={openWindowsSoundSettings}
        onTestShortcut={() => {
          void (async () => {
            shortcutCaptureRef.current = { arming: false, active: false, keys: [], message: "" };
            setShortcutCapture(shortcutCaptureRef.current);
            shortcutPressedCodesRef.current.clear();
            focusedPressedKeysRef.current.clear();
            focusedCapturedKeysRef.current.clear();
            if (shortcutTestTimerRef.current !== null) {
              window.clearTimeout(shortcutTestTimerRef.current);
              shortcutTestTimerRef.current = null;
            }
            setShortcutTest({
              active: true,
              detected: false,
              message: "Arming the Windows shortcut hook...",
            });
            try {
              // These commands mutate the same native hook state. They must
              // complete in order or a late capture cancellation can silently
              // pause the test that was just registered.
              await cancelShortcutCapture();
              const status = await registerSetupShortcut(settingsDraft.hotkey);
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
            } catch (error: unknown) {
              void setShortcutTestActive(false);
              setShortcutTest({
                active: false,
                detected: false,
                message: stringifyError(error),
              });
            }
          })();
        }}
        onRecordShortcut={() => {
          if (shortcutTestTimerRef.current !== null) {
            window.clearTimeout(shortcutTestTimerRef.current);
            shortcutTestTimerRef.current = null;
          }
          void setShortcutTestActive(false);
          void cancelShortcutCapture();
          shortcutPressedCodesRef.current.clear();
          focusedPressedKeysRef.current.clear();
          focusedCapturedKeysRef.current.clear();
          shortcutCaptureRef.current = {
            arming: false,
            active: true,
            keys: [],
            message: "Hold the keys you want together. They will light up immediately.",
          };
          setShortcutCapture(shortcutCaptureRef.current);
          setShortcutTest({
            active: false,
            detected: false,
            message: "Recording a new shortcut…",
          });
        }}
        onCancelShortcutTest={() => {
          if (shortcutTestTimerRef.current !== null) {
            window.clearTimeout(shortcutTestTimerRef.current);
            shortcutTestTimerRef.current = null;
          }
          void cancelShortcutCapture();
          shortcutCaptureRef.current = { arming: false, active: false, keys: [], message: "" };
          setShortcutCapture(shortcutCaptureRef.current);
          shortcutPressedCodesRef.current.clear();
          focusedPressedKeysRef.current.clear();
          focusedCapturedKeysRef.current.clear();
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
          void cancelShortcutCapture();
          shortcutCaptureRef.current = { arming: false, active: false, keys: [], message: "" };
          setShortcutCapture(shortcutCaptureRef.current);
          shortcutPressedCodesRef.current.clear();
          focusedPressedKeysRef.current.clear();
          focusedCapturedKeysRef.current.clear();
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
            const result = await injectOnboardingSample();
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
        onSelectModel={(modelId) => {
          const current = settingsRef.current;
          if (!current) return;
          const selected = {
            ...current,
            activeModelId: modelId,
            onboardingComplete: false,
            onboardingVersion: "",
            audioCalibration: null,
          };
          setSettingsDraft(selected);
          void saveSettings(selected)
            .then((next) => {
              setSnapshot(next);
              setSettingsDraft(next.settings);
            })
            .catch((error: unknown) => {
              setNotice({ tone: "error", message: stringifyError(error) });
            });
        }}
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
              const message = await copyText(sessionDisplayText(session));
              setNotice({ tone: "success", message });
            }}
          />
        ) : null}
        {activeTab === "history" ? (
          <HistoryPanel
            sessions={snapshot.sessions}
            onCopy={async (session) => {
              const message = await copyText(sessionDisplayText(session));
              setNotice({ tone: "success", message });
            }}
            onInject={async (session) => {
              const showingPolish = Boolean(
                session.preferPolished && session.polishedText?.trim(),
              );
              if (showingPolish) {
                const preview = session.polishedText!.trim();
                const confirmed = window.confirm(
                  `Paste the AI-edited text into the focused app?\n\n${preview.slice(0, 500)}${
                    preview.length > 500 ? "…" : ""
                  }`,
                );
                if (!confirmed) {
                  return;
                }
              }
              const result = await injectSession(session.id, showingPolish);
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
            onPolish={async (session) => {
              try {
                const next = await polishSession(session.id);
                setSnapshot(next);
                setNotice({ tone: "success", message: "AI polish applied." });
              } catch (error) {
                setNotice({
                  tone: "error",
                  message: error instanceof Error ? error.message : String(error),
                });
              }
            }}
            onUndoAiEdit={async (session) => {
              const next = await setSessionPreferPolished(session.id, false);
              setSnapshot(next);
              setNotice({ tone: "success", message: "AI edit undone." });
            }}
            onRedoAiEdit={async (session) => {
              const next = await setSessionPreferPolished(session.id, true);
              setSnapshot(next);
              setNotice({ tone: "success", message: "AI edit restored." });
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
            shortcutCapture={shortcutCapture}
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
              if (shortcutTestTimerRef.current !== null) {
                window.clearTimeout(shortcutTestTimerRef.current);
              }
              shortcutTestTimerRef.current = window.setTimeout(() => {
                shortcutTestTimerRef.current = null;
                void setShortcutTestActive(false);
                setShortcutTest({
                  active: false,
                  detected: false,
                  message: "Shortcut was not detected. Record another chord or retry.",
                });
              }, 15_000);
              setShortcutTest({
                active: true,
                detected: false,
                message: "Press your dictation shortcut…",
              });
            }}
            onRecordShortcut={() => {
              if (shortcutTestTimerRef.current !== null) {
                window.clearTimeout(shortcutTestTimerRef.current);
                shortcutTestTimerRef.current = null;
              }
              // Suppress the currently registered global chord while the
              // focused recorder is learning its replacement. Native key
              // events still light the keys if the old chord is pressed.
              void setShortcutTestActive(true);
              shortcutPressedCodesRef.current.clear();
              focusedPressedKeysRef.current.clear();
              focusedCapturedKeysRef.current.clear();
              shortcutCaptureRef.current = {
                arming: false,
                active: true,
                keys: [],
                message: "Hold any modifier chord together, then release every key to save it.",
              };
              setShortcutCapture(shortcutCaptureRef.current);
              setShortcutTest({ active: false, detected: false, message: "" });
            }}
            onCancelShortcutCapture={() => {
              void setShortcutTestActive(false);
              focusedPressedKeysRef.current.clear();
              focusedCapturedKeysRef.current.clear();
              shortcutCaptureRef.current = {
                arming: false,
                active: false,
                keys: [],
                message: "Shortcut recording cancelled.",
              };
              setShortcutCapture(shortcutCaptureRef.current);
            }}
            onShortcutChange={(hotkey) => {
              if (shortcutTestTimerRef.current !== null) {
                window.clearTimeout(shortcutTestTimerRef.current);
                shortcutTestTimerRef.current = null;
              }
              void setShortcutTestActive(false);
              focusedPressedKeysRef.current.clear();
              focusedCapturedKeysRef.current.clear();
              shortcutCaptureRef.current = { arming: false, active: false, keys: [], message: "" };
              setShortcutCapture(shortcutCaptureRef.current);
              setShortcutTest({
                active: false,
                detected: false,
                message: "Save settings to activate this shortcut.",
              });
              setSettingsDraft({ ...settingsDraft, hotkey });
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
            polishInventory={polishInventory}
            polishSetupBusy={polishSetupBusy}
            polishSetupMessage={polishSetupMessage}
            onEnsurePolishRuntime={async () => {
              if (!settingsDraft) return;
              setPolishSetupBusy(true);
              setPolishSetupMessage("Downloading local editor runtime if needed…");
              try {
                await ensurePolishRuntime(settingsDraft.polishModel || "qwen2.5-0.5b");
                setPolishInventory(await getPolishInventory());
                setPolishSetupMessage("Local editor is ready.");
                setNotice({ tone: "success", message: "Local AI editor is ready." });
              } catch (error: unknown) {
                const message = stringifyError(error);
                setPolishSetupMessage(message);
                setNotice({ tone: "error", message });
              } finally {
                setPolishSetupBusy(false);
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
                    current
                      ? {
                          ...current,
                          activeModelId: modelId,
                          modelSelectionMode: "manual",
                        }
                      : current,
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
  const [shortcutArmed, setShortcutArmed] = useState(false);
  const [liveTranscript, setLiveTranscript] = useState<{
    sessionId: string | null;
    phase: "idle" | "partial" | "stable" | "final" | "error";
    stableText: string;
    partialText: string;
    latencyMs: number | null;
  }>({
    sessionId: null,
    phase: "idle",
    stableText: "",
    partialText: "",
    latencyMs: null,
  });
  const shortcutKeysDownRef = useRef(new Set<number>());

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
          if (event.payload.phase === "listening" && event.payload.recording) {
            setLiveTranscript({
              sessionId: event.payload.recording.id,
              phase: "idle",
              stableText: "",
              partialText: "",
              latencyMs: null,
            });
          } else if (event.payload.phase === "idle") {
            setLiveTranscript((current) => ({ ...current, phase: "idle" }));
          }
          // "Set down in Notepad" — named from where the text actually landed.
          const target = event.payload.result?.injection?.targetProcessName;
          if (target) setHostApp(target);
        },
      );
      if (cancelled) unlisten();
      else unlisteners.push(unlisten);

      const unlistenTranscript = await listen<LiveTranscriptEvent>(
        "atmospeak://live-transcript",
        (event) => {
          const update = event.payload;
          setLiveTranscript((current) => {
            const sameSession = current.sessionId === update.sessionId;
            const stableText = update.stableText
              ? update.stableText
              : sameSession
                ? current.stableText
                : "";
            return {
              sessionId: update.sessionId,
              phase: update.stableText ? "stable" : "partial",
              stableText,
              partialText: update.partialText,
              latencyMs: update.firstPartialLatencyMs,
            };
          });
        },
      );
      if (cancelled) unlistenTranscript();
      else unlisteners.push(unlistenTranscript);

      // Appearance and hotkey changes are made in the hub's window, not this one.
      const unlistenSettings = await listen<AppSettings>(
        "atmospeak://settings-changed",
        (event) => setSettings(event.payload),
      );
      if (cancelled) unlistenSettings();
      else unlisteners.push(unlistenSettings);

      // Shortcut status is the native source of truth. Following it here keeps
      // the label honest even if registration completes before this WebView
      // subscribes to the broader settings event.
      const unlistenShortcut = await listen<ShortcutStatus>(
        "wind-speak://shortcut-status",
        (event) => {
          if (!event.payload.registered || !event.payload.hotkey) return;
          setSettings((current) =>
            current ? { ...current, hotkey: event.payload.hotkey } : current,
          );
        },
      );
      if (cancelled) unlistenShortcut();
      else unlisteners.push(unlistenShortcut);

      const unlistenShortcutKey = await listen<ShortcutKeyEvent>(
        "atmospeak://shortcut-key",
        (event) => {
          if (event.payload.pressed) {
            shortcutKeysDownRef.current.add(event.payload.code);
          } else {
            shortcutKeysDownRef.current.delete(event.payload.code);
          }
          setShortcutArmed(shortcutKeysDownRef.current.size > 0);
        },
      );
      if (cancelled) unlistenShortcutKey();
      else
        unlisteners.push(() => {
          shortcutKeysDownRef.current.clear();
          setShortcutArmed(false);
          unlistenShortcutKey();
        });

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
      // drag, so settle briefly before writing. If the OS left it off-screen,
      // the backend returns a clamped position — apply it so the orb springs
      // back into reach instead of staying stranded until a tray reset.
      let moveTimer: number | undefined;
      const unlistenMoved = await getCurrentWindow().onMoved(({ payload }) => {
        if (suppressPositionSaveRef.current) return;
        window.clearTimeout(moveTimer);
        moveTimer = window.setTimeout(() => {
          void saveOverlayPosition(payload.x, payload.y)
            .then(([x, y]) => {
              if (x === payload.x && y === payload.y) return;
              // Suppress the echo from our own settle so we don't write twice.
              suppressPositionSaveRef.current = true;
              void getCurrentWindow()
                .setPosition(new PhysicalPosition(x, y))
                .finally(() => {
                  window.setTimeout(() => {
                    suppressPositionSaveRef.current = false;
                  }, 500);
                });
            })
            .catch(() => undefined);
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
      if (phase === "idle") setElapsedSeconds(0);
      return undefined;
    }
    const startedAt = new Date(recording.startedAt).getTime();
    const interval = window.setInterval(() => {
      setElapsedSeconds(Math.max(0, (Date.now() - startedAt) / 1000));
    }, 500);
    return () => window.clearInterval(interval);
  }, [phase, recording]);

  return (
    <RecorderOverlay
      recording={recording}
      elapsedSeconds={elapsedSeconds}
      busy={phase === "finalizing"}
      phase={phase}
      modelStatus={modelStatus}
      notice={dragDiagnostic || message}
      liveTranscript={liveTranscript}
      inputLevel={level}
      inputBands={[]}
      bubbleSize="medium"
      bubbleOpacity={1}
      hostApp={hostApp}
      hotkeyLabel={settings?.hotkey ?? "your shortcut"}
      shortcutArmed={shortcutArmed}
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
