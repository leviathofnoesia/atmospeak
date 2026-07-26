import {
  CheckCircle2,
  Clipboard,
  Keyboard,
  Palette,
  Radio,
  RotateCw,
} from "lucide-react";
import type { ReactNode } from "react";
import type {
  AppSettings,
  MicrophoneInfo,
  ShortcutStatus,
  UpdateCheckResult,
  UpdateStatus,
} from "../types/dictation";
import { shortcutOptions } from "../panelOptions";
import { PanelTitle } from "./PanelTitle";
import { ToggleRow } from "./ToggleRow";

interface ShortcutTestState {
  active: boolean;
  detected: boolean;
  message: string;
}

interface SettingsPanelProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  microphones: MicrophoneInfo[];
  shortcutStatus: ShortcutStatus | null;
  shortcutTest: ShortcutTestState;
  onTestShortcut: () => void;
  onToggleShortcutsPaused: () => Promise<void>;
  onShowFloatingControl: () => Promise<void>;
  onResetDockPosition: () => Promise<void>;
  onRerunOnboarding: () => Promise<void>;
  onSave: () => Promise<void>;
  updateStatus: UpdateStatus;
  updateResult: UpdateCheckResult | null;
  onCheckUpdates: () => Promise<void>;
  onInstallUpdate: () => Promise<void>;
  advanced: ReactNode;
  modelManagement: ReactNode;
}

export function SettingsPanel({
  settings,
  setSettings,
  microphones,
  shortcutStatus,
  shortcutTest,
  onTestShortcut,
  onToggleShortcutsPaused,
  onShowFloatingControl,
  onResetDockPosition,
  onRerunOnboarding,
  onSave,
  updateStatus,
  updateResult,
  onCheckUpdates,
  onInstallUpdate,
  advanced,
  modelManagement,
}: SettingsPanelProps) {
  return (
    <div>
      <div className="hub__head">
        <div className="kick">P.05 / Settings — tune the room</div>
        <h1>Quiet by <em>default.</em></h1>
      </div>
      <section className="settings-panel">
      <PanelTitle icon={<Keyboard size={22} />} title="Input" />
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
          aria-label="Shortcut"
          value={settings.hotkey}
          onChange={(event) => setSettings({ ...settings, hotkey: event.currentTarget.value })}
        >
          {shortcutOptions.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>Mode</span>
        <select
          value={settings.mode}
          onChange={(event) =>
            setSettings({
              ...settings,
              mode: event.currentTarget.value as AppSettings["mode"],
            })
          }
        >
          <option value="pushToTalk">Push to talk</option>
          <option value="toggle">Toggle</option>
        </select>
      </label>
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
            setSettings({ ...settings, transcriptRetentionDays: Number(event.currentTarget.value) })
          }
        >
          <option value={1}>1 day</option>
          <option value={7}>7 days</option>
          <option value={30}>30 days</option>
          <option value={90}>90 days</option>
          <option value={0}>Keep until deleted</option>
        </select>
      </label>

      {modelManagement}

      <PanelTitle icon={<Palette size={22} />} title="Companion" />
      <p className="muted">How the floating dock looks and moves. Changes apply on save.</p>
      <label>
        <span>Accent pigment</span>
        <select
          value={settings.accent}
          onChange={(event) =>
            setSettings({ ...settings, accent: event.currentTarget.value as AppSettings["accent"] })
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
            setSettings({ ...settings, dockShape: event.currentTarget.value as AppSettings["dockShape"] })
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
            setSettings({ ...settings, waveStyle: event.currentTarget.value as AppSettings["waveStyle"] })
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
            setSettings({ ...settings, dockTheme: event.currentTarget.value as AppSettings["dockTheme"] })
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
            setSettings({ ...settings, motion: event.currentTarget.value as AppSettings["motion"] })
          }
        >
          <option value="lively">Lively</option>
          <option value="calm">Calm</option>
        </select>
      </label>

      <div className="settings-actions">
        <button type="button" className="button button--primary" onClick={() => void onSave()}>
          Save settings
        </button>
        <button type="button" className="button button--ghost" onClick={onTestShortcut}>
          Test active shortcut
        </button>
        <button type="button" className="button button--ghost" onClick={() => void onToggleShortcutsPaused()}>
          {shortcutStatus?.paused ? "Resume shortcuts" : "Pause shortcuts"}
        </button>
        <button type="button" className="button button--ghost" onClick={() => void onShowFloatingControl()}>
          Show floating control
        </button>
        <button type="button" className="button button--ghost" onClick={() => void onResetDockPosition()}>
          Reset dock position
        </button>
        <button type="button" className="button button--ghost" onClick={() => void onRerunOnboarding()}>
          Run onboarding
        </button>
      </div>

      <p className="muted">
        {shortcutStatus?.message ?? "Shortcut status unknown."}
        {shortcutTest.active ? ` · Testing… ${shortcutTest.message}` : null}
        {shortcutTest.detected ? ` · ${shortcutTest.message}` : null}
      </p>

      <PanelTitle icon={<CheckCircle2 size={22} />} title="Updates" />
      <div className="settings-actions">
        <button type="button" className="button button--ghost" onClick={() => void onCheckUpdates()}>
          Check for updates
        </button>
        {updateStatus === "available" ? (
          <button type="button" className="button button--primary" onClick={() => void onInstallUpdate()}>
            Install update
          </button>
        ) : null}
      </div>
      <p className="muted">{updateResult?.message ?? "Updates use the Tauri updater when configured."}</p>

      <details className="advanced-disclosure">
        <summary>Advanced diagnostics</summary>
        {advanced}
      </details>
      </section>
    </div>
  );
}
