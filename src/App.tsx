import { emit, listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import {
  BookOpen,
  CheckCircle2,
  Clipboard,
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
  Trash2,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import "./App.css";
import { RecorderOverlay } from "./components/RecorderOverlay";
import { StatusLed } from "./components/StatusLed";
import {
  cancelRecording,
  deleteDictionaryEntry,
  deleteSnippet,
  getAppSnapshot,
  checkForUpdates,
  downloadAndInstallUpdate,
  getModelInventory,
  hasTauriRuntime,
  getModelStatus,
  getRecordingLevel,
  getShortcutStatus,
  injectText,
  listMicrophones,
  saveSettings,
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
  RecordingStarted,
  ShortcutStatus,
  Snippet,
  TranscriptSession,
  UpdateCheckResult,
  UpdateStatus,
} from "./types/dictation";

const onboardingVersion = "desktop-parity-v2";
const shortcutOptions = [
  "Ctrl+Win+Space",
  "Ctrl+Alt+Space",
  "Ctrl+Shift+Space",
  "Ctrl+Alt+D",
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
  const [activeTab, setActiveTab] = useState<HubTab>("home");
  const [recording, setRecording] = useState<RecordingStarted | null>(null);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [recordingLevel, setRecordingLevel] = useState(0);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<AppNotice>({
    tone: "neutral",
    message: "Wind Speak is standing by.",
  });
  const [shortcutTest, setShortcutTest] = useState<ShortcutTestState>({
    active: false,
    detected: false,
    message: "Shortcut test is idle.",
  });
  const [dictionaryDraft, setDictionaryDraft] = useState({ phrase: "", replacement: "" });
  const [snippetDraft, setSnippetDraft] = useState({ trigger: "", body: "" });
  const recordingRef = useRef<RecordingStarted | null>(null);
  const busyRef = useRef(false);
  const settingsRef = useRef<AppSettings | null>(null);
  const shortcutTestRef = useRef(shortcutTest);

  const refresh = useCallback(async () => {
    const [
      nextSnapshot,
      nextMicrophones,
      nextModelStatus,
      nextModelInventory,
      nextShortcutStatus,
    ] = await Promise.all([
      getAppSnapshot(),
      listMicrophones(),
      getModelStatus(),
      getModelInventory(),
      getShortcutStatus(),
    ]);
    setSnapshot(nextSnapshot);
    setSettingsDraft(nextSnapshot.settings);
    setMicrophones(nextMicrophones);
    setModelStatus(nextModelStatus);
    setModelInventory(nextModelInventory);
    setShortcutStatus(nextShortcutStatus);
  }, []);

  const refreshSnapshotOnly = useCallback(async () => {
    const nextSnapshot = await getAppSnapshot();
    setSnapshot(nextSnapshot);
    setSettingsDraft(nextSnapshot.settings);
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
    refresh().catch((error: unknown) => {
      setNotice({ tone: "error", message: stringifyError(error) });
    });
  }, [refresh]);

  useEffect(() => {
    if (recording === null) {
      setElapsedSeconds(0);
      setRecordingLevel(0);
      return undefined;
    }

    const startedAt = new Date(recording.startedAt).getTime();
    const interval = window.setInterval(() => {
      setElapsedSeconds(Math.max(0, (Date.now() - startedAt) / 1000));
    }, 500);

    return () => window.clearInterval(interval);
  }, [recording]);

  useEffect(() => {
    if (recording === null) {
      return undefined;
    }

    let cancelled = false;
    const pollLevel = () => {
      getRecordingLevel()
        .then((level) => {
          if (!cancelled) {
            setRecordingLevel(level);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setRecordingLevel(0);
          }
        });
    };

    pollLevel();
    const interval = window.setInterval(pollLevel, 120);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [recording]);

  const handleToggleRecording = useCallback(async () => {
    if (busyRef.current) {
      return;
    }

    setBusy(true);
    try {
      if (recordingRef.current === null) {
        const started = await startRecording();
        setRecording(started);
        setNotice({ tone: "success", message: `Recording from ${started.microphoneName}.` });
      } else {
        setRecording(null);
        setNotice({ tone: "neutral", message: "Transcribing locally..." });
        const result = await stopRecording();
        await refreshSnapshotOnly();
        setNotice({
          tone: result.injection?.injected ? "success" : "neutral",
          message: result.injection?.message ?? "Transcript saved to history.",
        });
        setActiveTab("history");
      }
    } catch (error: unknown) {
      setRecording(null);
      await refresh().catch(() => undefined);
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusy(false);
    }
  }, [refresh, refreshSnapshotOnly]);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    const payload = {
      recording,
      elapsedSeconds,
      busy,
      modelStatus,
      shortcutStatus,
      notice,
      recordingLevel,
    };
    void emit("wind-speak://dictation-state", payload);
    return undefined;
  }, [busy, elapsedSeconds, modelStatus, notice, recording, recordingLevel, shortcutStatus]);

  const handleCancel = useCallback(async () => {
    if (busyRef.current) {
      return;
    }

    setBusy(true);
    try {
      await cancelRecording();
      setRecording(null);
      setNotice({ tone: "neutral", message: "Recording cancelled." });
    } catch (error: unknown) {
      setNotice({ tone: "error", message: stringifyError(error) });
    } finally {
      setBusy(false);
    }
  }, []);

  const armShortcutTest = useCallback(() => {
    const label = shortcutStatus?.hotkey || settingsRef.current?.hotkey || "the active shortcut";
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
    }, 8000);
  }, [shortcutStatus?.hotkey]);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    let removeShortcutListener: (() => void) | undefined;
    let removeOverlayListener: (() => void) | undefined;
    let removeShortcutStatusListener: (() => void) | undefined;
    let removeOverlayVisibilityListener: (() => void) | undefined;
    listen<string>("wind-speak://shortcut", (event) => {
      const action = event.payload;
      if (shortcutTestRef.current.active) {
        if (action === "pressed" || action === "toggle") {
          const label = shortcutStatus?.hotkey || settingsRef.current?.hotkey || "shortcut";
          setShortcutTest({
            active: false,
            detected: true,
            message: `${label} detected by the desktop runtime.`,
          });
          setNotice({ tone: "success", message: `${label} detected.` });
        }
        return;
      }

      const mode = settingsRef.current?.mode ?? "toggle";
      const activeRecording = recordingRef.current;
      if (mode === "pushToTalk") {
        if (action === "pressed" && activeRecording === null) {
          void handleToggleRecording();
        }
        if (action === "released" && activeRecording !== null) {
          void handleToggleRecording();
        }
        if (action === "toggle") {
          void handleToggleRecording();
        }
        return;
      }

      if (action === "pressed" || action === "toggle") {
        void handleToggleRecording();
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
        void handleToggleRecording();
      }
      if (event.payload === "cancel") {
        void handleCancel();
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
    };
  }, [handleCancel, handleToggleRecording, shortcutStatus?.hotkey]);

  const handleSaveSettings = async () => {
    if (settingsDraft === null) {
      return;
    }

    setBusy(true);
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
      setBusy(false);
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

  const recentSession = snapshot?.sessions[0] ?? null;
  const readiness = useMemo(() => {
    if (modelStatus?.ready) {
      return { tone: "good" as const, label: "Offline engine ready" };
    }
    return { tone: "warn" as const, label: "Runtime incomplete" };
  }, [modelStatus]);

  if (snapshot === null || settingsDraft === null) {
    return (
      <main className="boot">
        <div className="boot__mark">WS</div>
        <p>Starting local command surface...</p>
      </main>
    );
  }

  if (
    !settingsDraft.onboardingComplete ||
    settingsDraft.onboardingVersion !== onboardingVersion
  ) {
    return (
      <Onboarding
        settings={settingsDraft}
        setSettings={setSettingsDraft}
        microphones={microphones}
        modelStatus={modelStatus}
        shortcutStatus={shortcutStatus}
        shortcutTest={shortcutTest}
        onTestShortcut={armShortcutTest}
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
            tone={shortcutStatus?.registered ? "good" : "warn"}
            label={shortcutStatus?.hotkey || "Shortcut unavailable"}
          />
        </div>
      </section>

      <RecorderOverlay
        recording={recording}
        elapsedSeconds={elapsedSeconds}
        busy={busy}
        modelStatus={modelStatus}
        hotkeyLabel={shortcutStatus?.hotkey || snapshot.settings.hotkey}
        notice={shortcutStatus?.registered ? undefined : shortcutStatus?.message}
        inputLevel={recordingLevel}
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
              busy={busy}
            />
          )}
          {activeTab === "history" && (
            <HistoryPanel
              sessions={snapshot.sessions}
              onInject={async (session) => {
                const result = await injectText(session.cleanedText);
                setNotice({ tone: "success", message: result.message });
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
              onTestShortcut={armShortcutTest}
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
            />
          )}
        </div>
      </section>
    </main>
  );
}

interface OverlayStatePayload {
  recording: RecordingStarted | null;
  elapsedSeconds: number;
  busy: boolean;
  modelStatus: ModelStatus | null;
  shortcutStatus: ShortcutStatus | null;
  notice: AppNotice;
  recordingLevel: number;
}

function OverlayWindow() {
  const [state, setState] = useState<OverlayStatePayload>({
    recording: null,
    elapsedSeconds: 0,
    busy: false,
    modelStatus: null,
    shortcutStatus: null,
    notice: { tone: "neutral", message: "Wind Speak is standing by." },
    recordingLevel: 0,
  });

  useEffect(() => {
    document.body.classList.add("is-overlay-window");
    return () => document.body.classList.remove("is-overlay-window");
  }, []);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return undefined;
    }

    let removeStateListener: (() => void) | undefined;
    listen<OverlayStatePayload>("wind-speak://dictation-state", (event) => {
      setState(event.payload);
    })
      .then((unlisten) => {
        removeStateListener = unlisten;
      })
      .catch(() => undefined);

    return () => removeStateListener?.();
  }, []);

  return (
    <main className="overlay-shell" data-tauri-drag-region>
      <RecorderOverlay
        recording={state.recording}
        elapsedSeconds={state.elapsedSeconds}
        busy={state.busy}
        modelStatus={state.modelStatus}
        hotkeyLabel={state.shortcutStatus?.hotkey || "BUTTON"}
        notice={state.notice.message}
        inputLevel={state.recordingLevel}
        onToggle={() => void emit("wind-speak://overlay-command", "toggle")}
        onCancel={() => void emit("wind-speak://overlay-command", "cancel")}
      />
    </main>
  );
}

interface HomePanelProps {
  snapshot: AppSnapshot;
  modelStatus: ModelStatus | null;
  recentSession: TranscriptSession | null;
  onStart: () => void;
  busy: boolean;
}

function HomePanel({ snapshot, modelStatus, recentSession, onStart, busy }: HomePanelProps) {
  return (
    <section className="panel-grid">
      <div className="hero-panel">
        <p className="eyebrow">Working mode</p>
        <h2>Hold the shortcut, speak, release. Wind Speak cleans and pastes locally.</h2>
        <button className="button button--primary" type="button" onClick={onStart} disabled={busy}>
          <Mic size={18} />
          Start dictation
        </button>
      </div>
      <MetricCard label="Sessions" value={snapshot.stats.totalSessions.toString()} />
      <MetricCard label="Words" value={snapshot.stats.totalWords.toString()} />
      <MetricCard
        label="WPM"
        value={Math.round(snapshot.stats.averageWordsPerMinute).toString()}
      />
      <div className="machine-card">
        <div className="machine-card__header">
          <Cpu size={20} />
          <h3>Offline engine</h3>
        </div>
        <p>{modelStatus?.message ?? "Checking model status..."}</p>
      </div>
      <div className="machine-card machine-card--wide">
        <div className="machine-card__header">
          <Database size={20} />
          <h3>Latest transcript</h3>
        </div>
        <p>{recentSession?.cleanedText ?? "History is empty. Your first transcript will appear here."}</p>
      </div>
    </section>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function HistoryPanel({
  sessions,
  onInject,
}: {
  sessions: TranscriptSession[];
  onInject: (session: TranscriptSession) => Promise<void>;
}) {
  return (
    <section className="list-panel">
      <PanelTitle icon={<History size={22} />} title="Transcript history" />
      {sessions.length === 0 ? (
        <EmptyState text="No transcripts yet. Start a recording from the floating control." />
      ) : (
        sessions.map((session) => (
          <article className="history-item" key={session.id}>
            <div>
              <span className="history-item__date">{formatDate(session.createdAt)}</span>
              <p>{session.cleanedText}</p>
              <small>
                {session.wordCount} words / {formatDuration(session.durationMs)}
              </small>
            </div>
            <button
              className="button button--ghost button--square"
              type="button"
              onClick={() => void onInject(session)}
              aria-label="Paste transcript again"
              title="Paste transcript again"
            >
              <Clipboard size={18} />
            </button>
          </article>
        ))
      )}
    </section>
  );
}

function DictionaryPanel({
  entries,
  draft,
  setDraft,
  onSubmit,
  onToggle,
  onDelete,
}: {
  entries: DictionaryEntry[];
  draft: { phrase: string; replacement: string };
  setDraft: (draft: { phrase: string; replacement: string }) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => Promise<void>;
  onToggle: (entry: DictionaryEntry) => Promise<void>;
  onDelete: (entry: DictionaryEntry) => Promise<void>;
}) {
  return (
    <section className="list-panel">
      <PanelTitle icon={<BookOpen size={22} />} title="Custom dictionary" />
      <form className="inline-form" onSubmit={(event) => void onSubmit(event)}>
        <input
          value={draft.phrase}
          onChange={(event) => setDraft({ ...draft, phrase: event.currentTarget.value })}
          placeholder="heard phrase"
        />
        <input
          value={draft.replacement}
          onChange={(event) => setDraft({ ...draft, replacement: event.currentTarget.value })}
          placeholder="replacement"
        />
        <button className="button button--primary" type="submit">
          Add
        </button>
      </form>
      {entries.map((entry) => (
        <EditableRow
          key={entry.id}
          title={entry.phrase}
          body={entry.replacement}
          enabled={entry.enabled}
          onToggle={() => void onToggle(entry)}
          onDelete={() => void onDelete(entry)}
        />
      ))}
    </section>
  );
}

function SnippetPanel({
  snippets,
  draft,
  setDraft,
  onSubmit,
  onToggle,
  onDelete,
}: {
  snippets: Snippet[];
  draft: { trigger: string; body: string };
  setDraft: (draft: { trigger: string; body: string }) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => Promise<void>;
  onToggle: (snippet: Snippet) => Promise<void>;
  onDelete: (snippet: Snippet) => Promise<void>;
}) {
  return (
    <section className="list-panel">
      <PanelTitle icon={<Scissors size={22} />} title="Voice snippets" />
      <form className="inline-form" onSubmit={(event) => void onSubmit(event)}>
        <input
          value={draft.trigger}
          onChange={(event) => setDraft({ ...draft, trigger: event.currentTarget.value })}
          placeholder="spoken trigger"
        />
        <input
          value={draft.body}
          onChange={(event) => setDraft({ ...draft, body: event.currentTarget.value })}
          placeholder="expanded text"
        />
        <button className="button button--primary" type="submit">
          Add
        </button>
      </form>
      {snippets.map((snippet) => (
        <EditableRow
          key={snippet.id}
          title={snippet.trigger}
          body={snippet.body}
          enabled={snippet.enabled}
          onToggle={() => void onToggle(snippet)}
          onDelete={() => void onDelete(snippet)}
        />
      ))}
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
  onTestShortcut,
  onComplete,
}: {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  modelStatus: ModelStatus | null;
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  onTestShortcut: () => void;
  onComplete: () => Promise<void>;
}) {
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
            <StatusLed tone={microphones.length > 0 ? "good" : "warn"} label="Microphone check" />
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
            <div className="mic-meter" aria-hidden="true">
              {Array.from({ length: 12 }, (_, index) => (
                <span key={index} />
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
          <article className="onboarding-step onboarding-step--accent">
            <StatusLed tone="good" label="Private by default" />
            <h2>First paste test</h2>
            <p>
              Open Notepad or any text field, press the shortcut, and Wind Speak will paste through
              the system clipboard after transcription.
            </p>
            <button
              className="button button--primary"
              type="button"
              onClick={() => void onComplete()}
              disabled={!modelStatus?.ready}
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
      <div className="model-grid">
        {modelInventory?.models.map((model) => (
          <div className="model-pill" key={model.id}>
            <strong>{model.label}</strong>
            <span>{model.installed ? (model.bundled ? "Bundled" : "Installed") : "Available later"}</span>
          </div>
        ))}
      </div>
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
  onTestShortcut,
  onSave,
  updateStatus,
  updateResult,
  onCheckUpdates,
  onInstallUpdate,
}: {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  onTestShortcut: () => void;
  onSave: () => Promise<void>;
  updateStatus: UpdateStatus;
  updateResult: UpdateCheckResult | null;
  onCheckUpdates: () => Promise<void>;
  onInstallUpdate: () => Promise<void>;
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
          <button className="button button--ghost" type="button" onClick={onTestShortcut}>
            <Keyboard size={18} />
            Test active shortcut
          </button>
          <p>{shortcutTest.message}</p>
        </div>
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
      <ToggleRow
        icon={<RotateCw size={18} />}
        label="Cleanup spoken punctuation and dictionary terms"
        checked={settings.cleanupEnabled}
        onChange={(cleanupEnabled) => setSettings({ ...settings, cleanupEnabled })}
      />
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
      <button className="button button--primary" type="button" onClick={() => void onSave()}>
        <CheckCircle2 size={18} />
        Save settings
      </button>
    </section>
  );
}

function ToggleRow({
  icon,
  label,
  checked,
  onChange,
}: {
  icon: ReactNode;
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="toggle-row">
      {icon}
      <span>{label}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
    </label>
  );
}

function EditableRow({
  title,
  body,
  enabled,
  onToggle,
  onDelete,
}: {
  title: string;
  body: string;
  enabled: boolean;
  onToggle: () => void;
  onDelete: () => void;
}) {
  return (
    <article className={clsx("editable-row", !enabled && "is-muted")}>
      <div>
        <strong>{title}</strong>
        <p>{body}</p>
      </div>
      <div className="row-actions">
        <button className="button button--ghost" type="button" onClick={onToggle}>
          {enabled ? "On" : "Off"}
        </button>
        <button
          className="button button--ghost button--square"
          type="button"
          onClick={onDelete}
          aria-label="Delete"
          title="Delete"
        >
          <Trash2 size={17} />
        </button>
      </div>
    </article>
  );
}

function PanelTitle({ icon, title }: { icon: ReactNode; title: string }) {
  return (
    <div className="panel-title">
      {icon}
      <h2>{title}</h2>
    </div>
  );
}

function EmptyState({ text }: { text: string }) {
  return <p className="empty-state">{text}</p>;
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
