import { CheckCircle2, Cpu } from "lucide-react";
import type {
  AppSettings,
  ModelDownloadProgress,
  ModelInventory,
  ModelStatus,
  StageMetrics,
} from "../types/dictation";
import { PanelTitle } from "./PanelTitle";
import { StatusLed } from "./StatusLed";
import { ToggleRow } from "./ToggleRow";

interface AdvancedPanelProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  modelStatus: ModelStatus | null;
  modelInventory: ModelInventory | null;
  modelDownload: ModelDownloadProgress | null;
  lastMetrics: StageMetrics | null;
  onSelectModel: (modelId: string) => void;
  onDownloadModel: (modelId: string) => Promise<void>;
  onCancelModelDownload: () => Promise<void>;
  onDeleteModel: (modelId: string) => Promise<void>;
  onSave: () => Promise<void>;
}

export function AdvancedPanel({
  settings,
  setSettings,
  modelStatus,
  modelInventory,
  modelDownload,
  lastMetrics,
  onSelectModel,
  onDownloadModel,
  onCancelModelDownload,
  onDeleteModel,
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
          Atmospeak ships with whisper.cpp and Base English. The resident host keeps the selected
          model warm, with automatic one-shot CLI fallback. Override paths only for custom builds.
        </p>
      </div>
      <div className="model-grid">
        {modelInventory?.models.map((model) => {
          const isActive =
            settings.activeModelId === model.id && !settings.advancedRuntimeEnabled;
          const downloading =
            modelDownload?.modelId === model.id &&
            ["starting", "downloading", "verifying"].includes(modelDownload.status);
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
              {!model.bundled ? (
                <div className="model-pill__actions">
                  {downloading ? (
                    <button type="button" className="button button--ghost" onClick={() => void onCancelModelDownload()}>
                      Cancel {modelDownload?.percent != null ? `${Math.round(modelDownload.percent)}%` : ""}
                    </button>
                  ) : model.installed ? (
                    <>
                      <button type="button" className="button button--ghost" onClick={() => onSelectModel(model.id)}>
                        {isActive ? "Selected" : "Use"}
                      </button>
                      <button type="button" className="button button--ghost" onClick={() => void onDeleteModel(model.id)}>
                        Delete
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="button button--ghost"
                      disabled={Boolean(modelDownload && ["starting", "downloading", "verifying"].includes(modelDownload.status))}
                      onClick={() => void onDownloadModel(model.id)}
                    >
                      Download
                    </button>
                  )}
                </div>
              ) : null}
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
