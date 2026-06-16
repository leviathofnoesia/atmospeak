import { CheckCircle2, Cpu } from "lucide-react";
import type { AppSettings, ModelInventory, ModelStatus } from "../types/dictation";
import { PanelTitle } from "./PanelTitle";
import { StatusLed } from "./StatusLed";
import { ToggleRow } from "./ToggleRow";

interface AdvancedPanelProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  modelStatus: ModelStatus | null;
  modelInventory: ModelInventory | null;
  onSave: () => Promise<void>;
}

export function AdvancedPanel({
  settings,
  setSettings,
  modelStatus,
  modelInventory,
  onSave,
}: AdvancedPanelProps) {
  return (
    <section className="settings-panel">
      <PanelTitle icon={<Cpu size={22} />} title="Advanced runtime" />
      <StatusLed tone={modelStatus?.ready ? "good" : "warn"} label={modelStatus?.message ?? "Checking"} />
      <div className="instruction-card">
        <h3>Bundled by default</h3>
        <p>
          Atmospeak ships with whisper.cpp and Base English. Override these paths only when
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
