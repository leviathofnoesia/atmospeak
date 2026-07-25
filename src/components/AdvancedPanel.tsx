import { CheckCircle2, Cpu } from "lucide-react";
import type { AppSettings, ModelInventory, ModelStatus, StageMetrics } from "../types/dictation";
import { PanelTitle } from "./PanelTitle";
import { StatusLed } from "./StatusLed";
import { ToggleRow } from "./ToggleRow";

interface AdvancedPanelProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  modelStatus: ModelStatus | null;
  modelInventory: ModelInventory | null;
  lastMetrics: StageMetrics | null;
  onSave: () => Promise<void>;
}

export function AdvancedPanel({
  settings,
  setSettings,
  modelStatus,
  modelInventory,
  lastMetrics,
  onSave,
}: AdvancedPanelProps) {
  return (
    <section className="settings-panel">
      <PanelTitle icon={<Cpu size={22} />} title="Advanced runtime" />
      <StatusLed
        tone={modelStatus?.ready ? "good" : "warn"}
        label={modelStatus?.message ?? "Checking"}
      />
      <div className="instruction-card">
        <h3>Bundled by default</h3>
        <p>
          Atmospeak ships with whisper.cpp CLI and Base English. Each utterance spawns the CLI
          process (multi-second latency is expected). Override paths only for custom local builds.
        </p>
      </div>
      <div className="model-grid">
        {modelInventory?.models.map((model) => {
          const isActive =
            modelInventory?.activeModelId === model.id && !settings.advancedRuntimeEnabled;
          return (
            <div
              key={model.id}
              className={`model-pill ${isActive ? "model-pill--active" : ""}`}
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
            </div>
          );
        })}
      </div>
      <ToggleRow
        icon={<Cpu size={18} />}
        label="Use advanced runtime override"
        checked={settings.advancedRuntimeEnabled}
        onChange={(advancedRuntimeEnabled) =>
          setSettings({ ...settings, advancedRuntimeEnabled })
        }
      />
      <label>
        <span>Advanced whisper-cli path</span>
        <input
          value={settings.advancedWhisperCliPath}
          disabled={!settings.advancedRuntimeEnabled}
          onChange={(event) =>
            setSettings({ ...settings, advancedWhisperCliPath: event.currentTarget.value })
          }
          placeholder="C:\\path\\to\\whisper-cli.exe"
        />
      </label>
      <label>
        <span>Advanced model path</span>
        <input
          value={settings.advancedModelPath}
          disabled={!settings.advancedRuntimeEnabled}
          onChange={(event) =>
            setSettings({ ...settings, advancedModelPath: event.currentTarget.value })
          }
          placeholder="C:\\path\\to\\ggml-base.en.bin"
        />
      </label>
      {lastMetrics ? (
        <div className="instruction-card">
          <h3>Last stage metrics</h3>
          <p>
            total {lastMetrics.totalMs}ms · capture_stop {lastMetrics.captureStopMs}ms · write{" "}
            {lastMetrics.writeMs}ms · asr {lastMetrics.asrMs}ms · cleanup {lastMetrics.cleanupMs}ms ·
            inject {lastMetrics.injectMs}ms · backend {lastMetrics.asrBackend}
          </p>
        </div>
      ) : null}
      <button type="button" className="button button--primary" onClick={() => void onSave()}>
        <CheckCircle2 size={16} />
        Save advanced settings
      </button>
    </section>
  );
}
