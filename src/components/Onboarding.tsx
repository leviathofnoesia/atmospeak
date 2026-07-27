import { useMemo, useState } from "react";
import type {
  AppSettings,
  MicrophoneInfo,
  ModelDownloadProgress,
  ModelInventory,
  ModelInventoryItem,
  ModelStatus,
  SoundCheckResult,
  ShortcutStatus,
} from "../types/dictation";
import { shortcutOptions } from "../panelOptions";
import { Aura } from "./Aura";
import "./Onboarding.css";

interface ShortcutTestState {
  active: boolean;
  detected: boolean;
  message: string;
}
interface MicCheckState {
  active: boolean;
  passed: boolean;
  level: number;
  message: string;
}
interface PasteTestState {
  running: boolean;
  passed: boolean;
  message: string;
}
interface ShortcutCaptureState {
  arming: boolean;
  active: boolean;
  keys: string[];
  message: string;
}
interface SoundCheckState {
  active: boolean;
  result: SoundCheckResult | null;
  message: string;
}

interface OnboardingProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  modelStatus: ModelStatus | null;
  modelInventory: ModelInventory | null;
  modelDownload: ModelDownloadProgress | null;
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  shortcutCapture: ShortcutCaptureState;
  micCheck: MicCheckState;
  soundCheck: SoundCheckState;
  onStartMicCheck: () => Promise<void>;
  onStopMicCheck: () => Promise<void>;
  onStartSoundCheck: () => Promise<void>;
  onFinishSoundCheck: () => Promise<void>;
  onOpenWindowsSoundSettings: () => Promise<void>;
  onTestShortcut: () => void;
  onRecordShortcut: () => void;
  onCancelShortcutTest: () => void;
  onShortcutChange: (hotkey: string) => void;
  pasteTest: PasteTestState;
  onPasteTest: () => Promise<void>;
  onSelectModel: (modelId: string) => void;
  onDownloadModel: (modelId: string) => Promise<void>;
  onCancelModelDownload: () => Promise<void>;
  onComplete: () => Promise<void>;
}

const STEPS = ["Welcome", "Microphone", "Voice model", "Shortcut", "Sound check", "Ready"];
const SC_TARGET = "The porcelain moon hums over the studio.";
const ONBOARDING_MODEL_IDS = new Set(["tiny.en", "base.en", "small.en"]);

const FALLBACK_MODELS: ModelInventoryItem[] = [
  { id: "tiny.en", label: "Swift", installed: false, bundled: false, path: null, sizeMb: 74 },
  { id: "base.en", label: "Balanced", installed: true, bundled: true, path: "bundled", sizeMb: 142 },
  { id: "small.en", label: "Faithful", installed: false, bundled: false, path: null, sizeMb: 466 },
];

const MODEL_COPY: Record<string, { tag: string; desc: string }> = {
  "tiny.en": { tag: "fast", desc: "Quick drafts, casual notes. Lightest footprint." },
  "base.en": { tag: "recommended", desc: "The everyday voice - accurate and responsive." },
  "small.en": { tag: "most accurate", desc: "Names, jargon, accents. Needs a little more room." },
  "medium.en": { tag: "high accuracy", desc: "A larger English model for accuracy-first dictation." },
  "distil-large-v3": { tag: "fast + accurate", desc: "Large-model accuracy distilled for faster English transcription." },
  "base": { tag: "multilingual", desc: "Auto-detect and non-English dictation with a local model." },
};

// ── small inline glyphs ──
function Glyph({ d, size = 16, sw = 2 }: { d: string; size?: number; sw?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={sw}
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={d} />
    </svg>
  );
}
const ICONS = {
  check: "M5 13l4 4L19 7",
  mic: "M9 3h6v11a3 3 0 0 1-6 0zM5 11a7 7 0 0 0 14 0M12 18v3",
  arrow: "M5 12h14M13 6l6 6-6 6",
  lock: "M8 11V8a4 4 0 0 1 8 0v3",
};

// live mic meter driven by the real input level
function MicMeter({ live, level }: { live: boolean; level: number }) {
  const bars = 38;
  const heights = useMemo(
    () =>
      Array.from({ length: bars }, (_, index) => {
        if (!live) return 6;
        const centerWeight = 0.45 + 0.55 * Math.sin(((index + 1) / (bars + 1)) * Math.PI);
        const texture = 0.82 + ((index * 17) % 11) / 30;
        return 6 + Math.max(0, Math.min(1, level)) * centerWeight * texture * 78;
      }),
    [level, live],
  );
  return (
    <div className="ob-meter" data-live={live ? "true" : "false"}>
      <span className="idle-note">microphone idle</span>
      {heights.map((height, i) => (
        <span className="bar" key={i} style={{ height: `${height.toFixed(1)}px` }} />
      ))}
    </div>
  );
}

export function Onboarding(props: OnboardingProps) {
  const {
    settings,
    setSettings,
    microphones,
    modelStatus,
    modelInventory,
    modelDownload,
    shortcutStatus,
    shortcutTest,
    shortcutCapture,
    micCheck,
    soundCheck,
    onStartMicCheck,
    onStopMicCheck,
    onStartSoundCheck,
    onFinishSoundCheck,
    onOpenWindowsSoundSettings,
    onTestShortcut,
    onRecordShortcut,
    onCancelShortcutTest,
    onShortcutChange,
    pasteTest,
    onPasteTest,
    onSelectModel,
    onDownloadModel,
    onCancelModelDownload,
    onComplete,
  } = props;

  const [step, setStepRaw] = useState(0);
  const [completionError, setCompletionError] = useState("");
  const [completing, setCompleting] = useState(false);
  const setStep = (n: number) => setStepRaw(Math.max(0, Math.min(STEPS.length - 1, n)));

  const modelReady = modelStatus?.ready ?? false;
  const inventoryChoices = modelInventory?.models.filter((model) => ONBOARDING_MODEL_IDS.has(model.id)) ?? [];
  const modelChoices = inventoryChoices.length ? inventoryChoices : FALLBACK_MODELS;
  const activeModel =
    modelChoices.find((model) => model.id === settings.activeModelId) ??
    modelChoices.find((model) => model.id === modelInventory?.activeModelId) ??
    modelChoices.find((model) => model.id === "base.en") ??
    modelChoices[0];
  const selectedModelReady = activeModel?.installed ?? false;
  const canContinue = (() => {
    if (step === 1) return micCheck.passed;
    if (step === 2) return modelReady && selectedModelReady;
    if (step === 3) return shortcutTest.detected;
    if (step === 4) return soundCheck.result?.passed ?? false;
    return true;
  })();
  const progressPct = (step / (STEPS.length - 1)) * 100;
  const selectedHotkeyChips = settings.hotkey.split("+").map((part) => part.trim());
  const hotkeyChips =
    shortcutCapture.active && shortcutCapture.keys.length
      ? shortcutCapture.keys
      : selectedHotkeyChips;
  const completeSetup = async () => {
    setCompleting(true);
    setCompletionError("");
    try {
      await onComplete();
    } catch (error) {
      setCompletionError(error instanceof Error ? error.message : String(error));
      setCompleting(false);
    }
  };

  return (
    <div className="ob-stage">
      <div className="ob-window">
        <span className="crop tl" /><span className="crop tr" />
        <span className="crop bl" /><span className="crop br" />

        {/* LEFT RAIL */}
        <aside className="ob-rail">
          <div className="ob-brand">
            <span className="ob-aura"><Aura size={40} active /></span>
            <span className="wm"><strong>Atmospeak</strong><span>First run · on device</span></span>
          </div>
          <nav className="ob-steps">
            {STEPS.map((s, i) => (
              <div key={s} className={`ob-step${i === step ? " active" : i < step ? " done" : ""}`}>
                <span className="num">{i < step ? <Glyph d={ICONS.check} size={14} /> : String(i).padStart(2, "0")}</span>
                <span className="lab">{s}</span>
              </div>
            ))}
          </nav>
          <div className="ob-foot">
            <div className="ob-folio">
              <span>P.0{step} / 0{STEPS.length - 1}</span>
              <span className="ob-barcode">
                {Array.from({ length: 16 }).map((_, i) => (
                  <i key={i} style={{ height: `${5 + ((i * 37) % 12)}px`, opacity: i % 3 ? 0.4 : 0.7 }} />
                ))}
              </span>
            </div>
          </div>
        </aside>

        {/* RIGHT CONTENT */}
        <main className="ob-main">
          <div className="ob-content">
            {step === 0 && (
              <div className="ob-fade">
                <div className="ob-hero-aura"><Aura size={132} active /></div>
                <div className="ob-kick">Welcome · v0.3.1 Recovery</div>
                <h1 className="ob-h">Speak. It listens.<br />It sets the words <em>down.</em></h1>
                <p className="ob-lede">
                  Atmospeak turns your voice into clean text wherever your cursor rests — transcribed entirely
                  on this device. No cloud, no account, nothing to forget. Let's take a quiet minute to set it up.
                </p>
              </div>
            )}

            {step === 1 && (
              <div className="ob-fade ob-mic">
                <div>
                  <div className="ob-kick">Step 01 · Permission</div>
                  <h1 className="ob-h">May I <em>listen?</em></h1>
                  <p className="ob-lede">
                    Atmospeak needs your microphone to hear you. It only ever captures while you hold your key —
                    never in the background.
                  </p>
                </div>
                <label className="ob-field">
                  <span>Input device</span>
                  <select
                    value={settings.microphoneName ?? ""}
                    onChange={(e) =>
                      setSettings({ ...settings, microphoneName: e.currentTarget.value || null })
                    }
                  >
                    {microphones.map((m) => (
                      <option key={m.name} value={m.name}>{m.name}{m.isDefault ? " (default)" : ""}</option>
                    ))}
                  </select>
                </label>
                <MicMeter live={micCheck.active} level={micCheck.level} />
                {!micCheck.passed && !micCheck.active ? (
                  <button className="pill-btn" onClick={() => void onStartMicCheck()} disabled={microphones.length === 0}>
                    <Glyph d={ICONS.mic} size={16} /> Check selected microphone
                  </button>
                ) : micCheck.active ? (
                  <button className="pill-btn ghost" onClick={() => void onStopMicCheck()}>Stop mic check</button>
                ) : (
                  <div className="ob-statline"><span className="ok"><Glyph d={ICONS.check} size={14} /></span> {micCheck.message}</div>
                )}
                <div className="ob-device-actions">
                  <span>{micCheck.message || "Choose the microphone you intend to dictate with."}</span>
                  <button
                    type="button"
                    className="ob-preset"
                    onClick={() => void onOpenWindowsSoundSettings()}
                  >
                    Windows sound settings
                  </button>
                </div>
                <div className="ob-priv">
                  <Glyph d={ICONS.lock} size={18} sw={1.8} />
                  <div><b>Private by design.</b> Audio is processed locally and discarded the moment your words are written. It never touches a server.</div>
                </div>
              </div>
            )}

            {step === 2 && (
              <div className="ob-fade">
                <div className="ob-kick">Step 02 · On-device model</div>
                <h1 className="ob-h">A voice model, <em>downloaded once.</em></h1>
                <p className="ob-lede">Pick the voice that fits how you work. It runs entirely offline.</p>
                <div className="ob-panel ob-models">
                  {modelChoices.map((m) => {
                    const copy = MODEL_COPY[m.id] ?? {
                      tag: m.bundled ? "bundled" : m.installed ? "installed" : "available",
                      desc: m.installed
                        ? "A local Whisper model ready to run offline."
                        : "Visible in inventory; install it before selecting.",
                    };
                    const selected = activeModel?.id === m.id;
                    const downloading =
                      modelDownload?.modelId === m.id &&
                      (modelDownload.status === "starting" ||
                        modelDownload.status === "downloading" ||
                        modelDownload.status === "verifying");
                    return (
                    <div
                      key={m.id}
                      className="ob-model"
                      aria-pressed={selected}
                      data-selected={selected ? "true" : "false"}
                      role={m.installed ? "button" : undefined}
                      tabIndex={m.installed ? 0 : undefined}
                      onClick={() => m.installed && onSelectModel(m.id)}
                      onKeyDown={(event) => {
                        if (m.installed && (event.key === "Enter" || event.key === " ")) {
                          event.preventDefault();
                          onSelectModel(m.id);
                        }
                      }}
                    >
                      <span className="radio" />
                      <span>
                        <span className="mname">{m.label}<span className="tag">{selected ? "selected" : copy.tag}</span></span>
                        <span className="mdesc">{copy.desc}</span>
                      </span>
                      <span className="msize">{m.sizeMb ? `${m.sizeMb} MB` : m.installed ? "local" : "not installed"}</span>
                      {!m.bundled ? (
                        <button
                          type="button"
                          className="ob-model-action"
                          disabled={Boolean(
                            modelDownload &&
                            !downloading &&
                            ["starting", "downloading", "verifying"].includes(modelDownload.status),
                          )}
                          onClick={(event) => {
                            event.stopPropagation();
                            if (downloading) {
                              void onCancelModelDownload();
                            } else if (m.installed) {
                              onSelectModel(m.id);
                            } else {
                              void onDownloadModel(m.id);
                            }
                          }}
                        >
                          {downloading ? "Cancel" : m.installed ? (selected ? "Selected" : "Use") : "Download"}
                        </button>
                      ) : null}
                    </div>
                    );
                  })}
                </div>
                <div className="ob-download">
                  <div className="row">
                    <span className="lbl">
                      {modelDownload?.message ??
                        (selectedModelReady && modelReady
                          ? "Ready · runs offline forever"
                          : "Download the selected model to continue.")}
                    </span>
                    <span className="pct">
                      {modelDownload?.percent != null
                        ? `${Math.round(modelDownload.percent)}%`
                        : selectedModelReady && modelReady
                          ? "100%"
                          : "…"}
                    </span>
                  </div>
                  <div className={`ob-dlbar${selectedModelReady && modelReady ? " done" : ""}`}>
                    <i
                      style={{
                        width: modelDownload?.percent != null
                          ? `${modelDownload.percent}%`
                          : selectedModelReady && modelReady
                            ? "100%"
                            : "0%",
                      }}
                    />
                  </div>
                </div>
              </div>
            )}

            {step === 3 && (
              <div className="ob-fade ob-keywrap">
                <div>
                  <div className="ob-kick">Step 03 · Your key</div>
                  <h1 className="ob-h">Choose your <em>key.</em></h1>
                  <p className="ob-lede">Record any modifier chord you prefer, then prove that the same chord works system-wide.</p>
                </div>
                <div className="ob-keyfield">
                  <span className="ob-keys" aria-live="polite" aria-label="Shortcut keys">
                    {hotkeyChips.map((key, index) => (
                      <kbd
                        key={`${key}-${index}`}
                        className={shortcutCapture.keys.includes(key) ? "is-down" : ""}
                      >
                        {key}
                      </kbd>
                    ))}
                  </span>
                  <span className="ob-keyactions">
                    <button
                      className={`ob-preset${shortcutCapture.active ? " is-recording" : ""}`}
                      type="button"
                      disabled={shortcutTest.active || shortcutCapture.active || shortcutCapture.arming}
                      onClick={onRecordShortcut}
                    >
                      {shortcutCapture.arming
                        ? "Arming..."
                        : shortcutCapture.active
                          ? "Recording keys..."
                          : "Record shortcut"}
                    </button>
                    <button
                      className="ob-preset"
                      type="button"
                      disabled={shortcutTest.active || shortcutCapture.active || shortcutCapture.arming}
                      onClick={onTestShortcut}
                    >
                      {shortcutTest.active
                        ? "Testing..."
                        : shortcutTest.detected
                          ? "Test again"
                          : "Test selected"}
                    </button>
                  </span>
                </div>
                <p className="ob-keyhint">
                  {shortcutCapture.message ||
                    shortcutTest.message ||
                    shortcutStatus?.message ||
                    "Record a chord, then test the selected shortcut."}
                </p>
                <div className="ob-kick">Quick picks</div>
                <div className="ob-presets">
                  {shortcutOptions.map((opt) => (
                    <button
                      key={opt}
                      className={`ob-preset${settings.hotkey === opt ? " sel" : ""}`}
                      type="button"
                      onClick={() => onShortcutChange(opt)}
                    >
                      {opt}
                    </button>
                  ))}
                </div>
                <div>
                  <div className="ob-kick" style={{ marginBottom: 12 }}>Gesture</div>
                  <div className="ob-modes">
                    <button className={`ob-mode${settings.mode === "pushToTalk" ? " sel" : ""}`} onClick={() => setSettings({ ...settings, mode: "pushToTalk" })}>Hold to talk</button>
                    <button className={`ob-mode${settings.mode === "toggle" ? " sel" : ""}`} onClick={() => setSettings({ ...settings, mode: "toggle" })}>Tap to toggle</button>
                  </div>
                </div>
              </div>
            )}

            {step === 4 && (
              <div className="ob-fade ob-sc">
                <div>
                  <div className="ob-kick">Step 04 · Sound check</div>
                  <h1 className="ob-h">Say this <em>line.</em></h1>
                  <p className="ob-lede">
                    A real local transcription confirms the microphone, model, and resident host together.
                  </p>
                </div>
                <div className="ob-sc-card">
                  <div className="strip"><span>READ ALOUD</span><span>◇ CALIBRATION</span></div>
                  <span className={`ob-sc-target${soundCheck.result?.passed ? " is-pass" : ""}`}>
                    {soundCheck.result?.transcript || SC_TARGET}
                  </span>
                </div>
                <div className="ob-sc-row">
                  {!soundCheck.result?.passed ? (
                    <button
                      className={`ob-hold${soundCheck.active ? " holding" : ""}`}
                      onPointerDown={(event) => {
                        event.currentTarget.setPointerCapture(event.pointerId);
                        void onStartSoundCheck();
                      }}
                      onPointerUp={() => void onFinishSoundCheck()}
                      onPointerCancel={() => void onFinishSoundCheck()}
                    >
                      <span className="dot" />{soundCheck.active ? "Listening…" : "Hold to read"}
                    </button>
                  ) : (
                    <div className="ob-sc-pass">
                      <span className="ok"><Glyph d={ICONS.check} size={13} /></span>
                      Heard clearly · host ASR {soundCheck.result.asrMs}ms
                    </div>
                  )}
                </div>
                {soundCheck.message ? <p className="ob-keyhint">{soundCheck.message}</p> : null}
              </div>
            )}

            {step === 5 && (
              <div className="ob-fade ob-ready">
                <div className="ob-ready-aura"><Aura size={120} active /></div>
                <div className="ob-kick">Ready</div>
                <h1 className="ob-h">The companion is <em>waiting.</em></h1>
                <p className="ob-lede">
                  It rests quietly at the edge of your screen. Reach for your key and it wakes; speak, release,
                  and your words are set down.
                </p>
                <div className="ob-ready-summary">
                  <div className="line"><span className="ok"><Glyph d={ICONS.check} size={12} /></span> Microphone <b>{soundCheck.result?.deviceName ?? "not checked"}</b></div>
                  <div className="line"><span className="ok"><Glyph d={ICONS.check} size={12} /></span> Resident ASR <b>{soundCheck.result?.asrBackend ?? "not checked"}</b> · SNR {soundCheck.result?.snrDb.toFixed(1) ?? "—"} dB</div>
                  <div className="line"><span className="ok"><Glyph d={ICONS.check} size={12} /></span> <b>{activeModel?.label ?? "Balanced"}</b> model · runs offline</div>
                  <div className="line"><span className="ok"><Glyph d={ICONS.check} size={12} /></span> Summon with <b>{selectedHotkeyChips.join(" ")}</b> · {settings.mode === "pushToTalk" ? "hold to talk" : "tap to toggle"}</div>
                </div>
                <div className="ob-ready-actions">
                  <button className="ob-preset" onClick={() => void onPasteTest()} disabled={pasteTest.running}>
                    {pasteTest.running ? "Testing paste…" : "Run paste test"}
                  </button>
                </div>
                {pasteTest.message && pasteTest.message !== "Paste test has not run yet." ? (
                  <p className="ob-keyhint">{pasteTest.message}</p>
                ) : null}
                {completionError ? <p className="ob-keyhint ob-error">{completionError}</p> : null}
              </div>
            )}
          </div>

          <div className="ob-nav">
            {step > 0 && (
              <button
                className="pill-btn ghost"
                type="button"
                onClick={() => {
                  if (step === 3) onCancelShortcutTest();
                  setStep(step - 1);
                }}
              >
                Back
              </button>
            )}
            <div className="ob-progress"><i style={{ width: `${progressPct}%` }} /></div>
            {step < STEPS.length - 1 ? (
              <>
                <button
                  className={`pill-btn${canContinue ? "" : " ghost"}`}
                  disabled={!canContinue}
                  onClick={() => {
                    if (step === 3) onCancelShortcutTest();
                    setStep(step + 1);
                  }}
                  style={canContinue ? {} : { opacity: 0.4, cursor: "not-allowed" }}
                >
                  {step === 0 ? "◇ Begin" : "Continue"} {canContinue && step > 0 ? <Glyph d={ICONS.arrow} size={15} /> : null}
                </button>
              </>
            ) : (
              <button
                className="pill-btn accent big"
                disabled={completing}
                onClick={() => void completeSetup()}
              >
                {completing ? "Finishing setup…" : "◇ Enter Atmospeak"}
              </button>
            )}
          </div>
        </main>
      </div>
      <div className="grain-overlay" />
    </div>
  );
}
