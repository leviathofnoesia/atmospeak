import {
  CheckCircle2,
  Clipboard,
  Database,
  Download,
  Keyboard,
  Radio,
  RotateCw,
  Sparkles,
  Zap,
} from "lucide-react";
import type {
  AppSettings,
  MicrophoneInfo,
  RuntimeEvent,
  ShortcutStatus,
  UpdateCheckResult,
  UpdateStatus,
} from "../types/dictation";
import { languageOptions, polishStyleOptions, shortcutOptions } from "../panelOptions";
import { PanelTitle } from "./PanelTitle";
import { RuntimeEventList } from "./RuntimeEventList";
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
}

export function SettingsPanel({
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
}: SettingsPanelProps) {
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
