import {
  CheckCircle2,
  Clipboard,
  Keyboard,
  Palette,
  Radio,
  RotateCw,
  Sparkles,
  Wrench,
} from "lucide-react";
import type { ReactNode } from "react";
import type {
  AppSettings,
  MicrophoneInfo,
  ModelInventoryItem,
  ShortcutStatus,
  UpdateCheckResult,
  UpdateStatus,
} from "../types/dictation";
import { shortcutOptions } from "../panelOptions";
import { PanelTitle } from "./PanelTitle";
import { PolishSettings } from "./PolishSettings";
import { ToggleRow } from "./ToggleRow";

interface ShortcutTestState {
  active: boolean;
  detected: boolean;
  message: string;
}

interface ShortcutCaptureState {
  arming: boolean;
  active: boolean;
  keys: string[];
  message: string;
}

const SETTINGS_SECTIONS = [
  { id: "settings-input", label: "Input" },
  { id: "settings-polish", label: "AI polish" },
  { id: "settings-models", label: "Models" },
  { id: "settings-transcription", label: "Transcription" },
  { id: "settings-companion", label: "Companion" },
  { id: "settings-tools", label: "Tools" },
  { id: "settings-updates", label: "Updates" },
  { id: "settings-advanced", label: "Advanced" },
] as const;

interface SettingsPanelProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  shortcutCapture: ShortcutCaptureState;
  dirty: boolean;
  saving: boolean;
  onTestShortcut: () => void;
  onRecordShortcut: () => void;
  onCancelShortcutCapture: () => void;
  onShortcutChange: (hotkey: string) => void;
  onToggleShortcutsPaused: () => Promise<void>;
  onShowFloatingControl: () => Promise<void>;
  onResetDockPosition: () => Promise<void>;
  onRerunOnboarding: () => Promise<void>;
  onSave: () => Promise<void>;
  onDiscard: () => void;
  updateStatus: UpdateStatus;
  updateResult: UpdateCheckResult | null;
  onCheckUpdates: () => Promise<void>;
  onInstallUpdate: () => Promise<void>;
  advanced: ReactNode;
  modelManagement: ReactNode;
  polishInventory?: ModelInventoryItem[];
  polishSetupBusy?: boolean;
  polishSetupMessage?: string;
  onEnsurePolishRuntime?: () => Promise<void>;
}

function scrollToSection(id: string) {
  const target = document.getElementById(id);
  target?.scrollIntoView({ behavior: "smooth", block: "start" });
}

export function SettingsPanel({
  settings,
  setSettings,
  microphones,
  shortcutStatus,
  shortcutTest,
  shortcutCapture,
  dirty,
  saving,
  onTestShortcut,
  onRecordShortcut,
  onCancelShortcutCapture,
  onShortcutChange,
  onToggleShortcutsPaused,
  onShowFloatingControl,
  onResetDockPosition,
  onRerunOnboarding,
  onSave,
  onDiscard,
  updateStatus,
  updateResult,
  onCheckUpdates,
  onInstallUpdate,
  advanced,
  modelManagement,
  polishInventory = [],
  polishSetupBusy = false,
  polishSetupMessage = "",
  onEnsurePolishRuntime,
}: SettingsPanelProps) {
  const shortcutKeys = (
    shortcutCapture.active && shortcutCapture.keys.length > 0
      ? shortcutCapture.keys
      : settings.hotkey.split("+")
  ).map((key) => key.trim());
  const shortcutSaved =
    shortcutStatus?.registered === true &&
    shortcutStatus.hotkey.toLowerCase() === settings.hotkey.toLowerCase();

  const footerStatus = saving
    ? "Saving…"
    : dirty
      ? "Unsaved changes"
      : "All changes saved";

  return (
    <div className="settings-view">
      <div className="settings-view__body">
        <div className="hub__head">
          <div className="kick">P.05 / Settings</div>
          <h1>Quiet by <em>default.</em></h1>
        </div>

        <nav className="settings-jump" aria-label="Settings sections">
          {SETTINGS_SECTIONS.map((section) => (
            <button
              key={section.id}
              type="button"
              className="settings-jump__chip"
              onClick={() => scrollToSection(section.id)}
            >
              {section.label}
            </button>
          ))}
        </nav>

        <section className="settings-panel">
          <div className="settings-section" id="settings-input">
            <PanelTitle icon={<Keyboard size={22} />} title="Input" />
            <label>
              <span>Microphone</span>
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
            <div className="settings-shortcut">
              <span className="settings-shortcut__label">Dictation shortcut</span>
              <div className="settings-shortcut__recorder">
                <span
                  className="settings-shortcut__keys"
                  aria-live="polite"
                  aria-label="Shortcut keys"
                >
                  {shortcutKeys.map((key, index) => (
                    <kbd
                      key={`${key}-${index}`}
                      className={shortcutCapture.keys.includes(key) ? "is-down" : ""}
                    >
                      {key}
                    </kbd>
                  ))}
                </span>
                <span className="settings-shortcut__actions">
                  {shortcutCapture.active ? (
                    <button
                      type="button"
                      className="button button--ghost"
                      onClick={onCancelShortcutCapture}
                    >
                      Cancel recording
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="button button--ghost"
                      disabled={shortcutTest.active}
                      onClick={onRecordShortcut}
                    >
                      Record any chord
                    </button>
                  )}
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={!shortcutSaved || shortcutCapture.active || shortcutTest.active}
                    onClick={onTestShortcut}
                  >
                    {shortcutTest.active ? "Listening..." : "Test active shortcut"}
                  </button>
                </span>
              </div>
              <p className="muted settings-shortcut__message">
                {shortcutCapture.message ||
                  (shortcutSaved
                    ? shortcutTest.message || shortcutStatus?.message
                    : "Save to activate and test this shortcut.")}
              </p>
              <div className="settings-shortcut__presets" aria-label="Shortcut quick picks">
                {shortcutOptions.map((option) => (
                  <button
                    type="button"
                    className={settings.hotkey === option ? "is-selected" : ""}
                    key={option}
                    onClick={() => onShortcutChange(option)}
                  >
                    {option}
                  </button>
                ))}
              </div>
            </div>
            <fieldset className="settings-gesture">
              <legend>Gesture</legend>
              <button
                type="button"
                className={settings.mode === "pushToTalk" ? "is-selected" : ""}
                onClick={() => setSettings({ ...settings, mode: "pushToTalk" })}
              >
                <strong>Hold</strong>
                <span>
                  Press and hold to record. Releasing transcribes and pastes automatically.
                </span>
              </button>
              <button
                type="button"
                className={settings.mode === "toggle" ? "is-selected" : ""}
                onClick={() => setSettings({ ...settings, mode: "toggle" })}
              >
                <strong>Tap</strong>
                <span>Press once to start and again to transcribe and paste.</span>
              </button>
            </fieldset>
            <ToggleRow
              icon={<Clipboard size={18} />}
              label="Auto-paste into focused app"
              checked={settings.autoInject}
              onChange={(autoInject) => setSettings({ ...settings, autoInject })}
            />
            <ToggleRow
              icon={<Clipboard size={18} />}
              label="Restore clipboard after paste"
              checked={settings.restoreClipboard}
              onChange={(restoreClipboard) => setSettings({ ...settings, restoreClipboard })}
            />
            <ToggleRow
              icon={<Radio size={18} />}
              label="Cleanup fillers and spoken punctuation"
              checked={settings.cleanupEnabled}
              onChange={(cleanupEnabled) => setSettings({ ...settings, cleanupEnabled })}
            />
            <ToggleRow
              icon={<RotateCw size={18} />}
              label="Start Atmospeak with Windows"
              checked={settings.startAtLogin}
              onChange={(startAtLogin) => setSettings({ ...settings, startAtLogin })}
            />
            <label>
              <span>Transcript retention</span>
              <select
                value={settings.transcriptRetentionDays}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    transcriptRetentionDays: Number(event.currentTarget.value),
                  })
                }
              >
                <option value={1}>1 day</option>
                <option value={7}>7 days</option>
                <option value={30}>30 days</option>
                <option value={90}>90 days</option>
                <option value={0}>Keep until deleted</option>
              </select>
            </label>
          </div>

          <div className="settings-section" id="settings-polish">
            <PanelTitle icon={<Sparkles size={22} />} title="AI polish" />
            <p className="muted">
              Optional rewrite before paste. Provider API keys save immediately to the OS keyring;
              other polish options need Save changes.
            </p>
            <PolishSettings
              settings={settings}
              setSettings={setSettings}
              polishInventory={polishInventory}
              polishSetupBusy={polishSetupBusy}
              polishSetupMessage={polishSetupMessage}
              onEnsurePolishRuntime={onEnsurePolishRuntime}
            />
          </div>

          <div className="settings-section" id="settings-models">
            {modelManagement}
          </div>

          <div className="settings-section" id="settings-transcription">
            <PanelTitle icon={<Radio size={22} />} title="Local transcription" />
            <p className="muted">
              Atmospeak can decode bounded segments while you speak. Live text stays in the
              companion and the final result is pasted only once.
            </p>
            <label>
              <span>Model selection</span>
              <select
                value={settings.modelSelectionMode}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    modelSelectionMode: event.currentTarget
                      .value as AppSettings["modelSelectionMode"],
                  })
                }
              >
                <option value="automatic">Automatic</option>
                <option value="manual">Manual</option>
              </select>
            </label>
            <label>
              <span>Transcription profile</span>
              <select
                value={settings.transcriptionProfile}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    transcriptionProfile: event.currentTarget
                      .value as AppSettings["transcriptionProfile"],
                  })
                }
              >
                <option value="balanced">Balanced</option>
                <option value="quality">Quality</option>
                <option value="speed">Speed</option>
              </select>
            </label>
            <label>
              <span>Acceleration</span>
              <select
                value={settings.accelerationPreference}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    accelerationPreference: event.currentTarget
                      .value as AppSettings["accelerationPreference"],
                  })
                }
              >
                <option value="auto">Auto (Vulkan, then CPU)</option>
                <option value="vulkan">Vulkan</option>
                <option value="cpu">CPU</option>
              </select>
            </label>
            <ToggleRow
              icon={<Radio size={18} />}
              label="Live preview in the companion"
              checked={settings.livePreviewEnabled}
              onChange={(livePreviewEnabled) =>
                setSettings({ ...settings, livePreviewEnabled })
              }
            />
          </div>

          <div className="settings-section" id="settings-companion">
            <PanelTitle icon={<Palette size={22} />} title="Companion" />
            <p className="muted">How the floating dock looks and moves. Changes apply on save.</p>
            <label>
              <span>Accent pigment</span>
              <select
                value={settings.accent}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    accent: event.currentTarget.value as AppSettings["accent"],
                  })
                }
              >
                <option value="dusk">Dusk</option>
                <option value="teal">Teal</option>
                <option value="lilac">Lilac</option>
              </select>
            </label>
            <label>
              <span>Resting shape</span>
              <select
                value={settings.dockShape}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    dockShape: event.currentTarget.value as AppSettings["dockShape"],
                  })
                }
              >
                <option value="orb">Orb</option>
                <option value="capsule">Capsule</option>
                <option value="tape">Tape</option>
              </select>
            </label>
            <label>
              <span>Voice wave</span>
              <select
                value={settings.waveStyle}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    waveStyle: event.currentTarget.value as AppSettings["waveStyle"],
                  })
                }
              >
                <option value="ribbon">Ribbon</option>
                <option value="bars">Bars</option>
                <option value="pulse">Pulse</option>
              </select>
            </label>
            <label>
              <span>Dock theme</span>
              <select
                value={settings.dockTheme}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    dockTheme: event.currentTarget.value as AppSettings["dockTheme"],
                  })
                }
              >
                <option value="dark">Smoked</option>
                <option value="light">Clear</option>
              </select>
            </label>
            <label>
              <span>Motion</span>
              <select
                value={settings.motion}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    motion: event.currentTarget.value as AppSettings["motion"],
                  })
                }
              >
                <option value="lively">Lively</option>
                <option value="calm">Calm</option>
              </select>
            </label>
          </div>

          <div className="settings-section" id="settings-tools">
            <PanelTitle icon={<Wrench size={22} />} title="Tools" />
            <p className="muted">
              {shortcutStatus?.message ?? "Shortcut status unknown."}
              {shortcutTest.active ? ` · Testing… ${shortcutTest.message}` : null}
              {shortcutTest.detected ? ` · ${shortcutTest.message}` : null}
            </p>
            <div className="settings-actions">
              <button
                type="button"
                className="button button--ghost"
                onClick={() => void onToggleShortcutsPaused()}
              >
                {shortcutStatus?.paused ? "Resume shortcuts" : "Pause shortcuts"}
              </button>
              <button
                type="button"
                className="button button--ghost"
                onClick={() => void onShowFloatingControl()}
              >
                Show floating control
              </button>
              <button
                type="button"
                className="button button--ghost"
                onClick={() => void onResetDockPosition()}
              >
                Reset dock position
              </button>
              <button
                type="button"
                className="button button--ghost"
                onClick={() => void onRerunOnboarding()}
              >
                Run onboarding
              </button>
            </div>
          </div>

          <div className="settings-section" id="settings-updates">
            <PanelTitle icon={<CheckCircle2 size={22} />} title="Updates" />
            <div className="settings-actions">
              <button
                type="button"
                className="button button--ghost"
                onClick={() => void onCheckUpdates()}
              >
                Check for updates
              </button>
              {updateStatus === "available" ? (
                <button
                  type="button"
                  className="button button--primary"
                  onClick={() => void onInstallUpdate()}
                >
                  Install update
                </button>
              ) : null}
            </div>
            <p className="muted">
              {updateResult?.message ?? "Updates use the Tauri updater when configured."}
            </p>
          </div>

          <div className="settings-section" id="settings-advanced">
            <details className="advanced-disclosure">
              <summary>Advanced diagnostics</summary>
              {advanced}
            </details>
          </div>
        </section>
      </div>

      <footer
        className={`settings-footer${dirty ? " settings-footer--dirty" : ""}`}
        aria-live="polite"
      >
        <p className="settings-footer__status">{footerStatus}</p>
        <div className="settings-footer__actions">
          {dirty ? (
            <button
              type="button"
              className="button button--ghost"
              disabled={saving}
              onClick={onDiscard}
            >
              Discard
            </button>
          ) : null}
          <button
            type="button"
            className="button button--primary"
            disabled={saving || !dirty}
            onClick={() => void onSave()}
          >
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      </footer>
    </div>
  );
}
