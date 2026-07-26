import { CheckCircle2, Cpu } from "lucide-react";
import type {
  AppSettings,
  ModelStatus,
  RuntimeEvent,
  StageMetrics,
} from "../types/dictation";
import { PanelTitle } from "./PanelTitle";
import { RuntimeEventList } from "./RuntimeEventList";
import { StatusLed } from "./StatusLed";
import { ToggleRow } from "./ToggleRow";

interface AdvancedPanelProps {
  settings: AppSettings;
  setSettings: (settings: AppSettings) => void;
  modelStatus: ModelStatus | null;
  lastMetrics: StageMetrics | null;
  runtimeEvents: RuntimeEvent[];
  onRunDiagnosticSoundCheck: () => Promise<void>;
  onSave: () => Promise<void>;
}

export function AdvancedPanel({
  settings,
  setSettings,
  modelStatus,
  lastMetrics,
  runtimeEvents,
  onRunDiagnosticSoundCheck,
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
      <div className="instruction-card">
        <h3>Selected input and paths</h3>
        <p>Microphone · {settings.microphoneName ?? "none selected"}</p>
        <p>Whisper CLI · {modelStatus?.whisperCliPath || "unavailable"}</p>
        <p>Model · {modelStatus?.modelPath || "unavailable"}</p>
      </div>
      <PanelTitle icon={<Cpu size={22} />} title="Runtime logs" />
      <RuntimeEventList events={runtimeEvents} />
      <button type="button" className="button button--ghost" onClick={() => void onRunDiagnosticSoundCheck()}>
        Run diagnostic sound check
      </button>
      <button type="button" className="button button--primary" onClick={() => void onSave()}>
        <CheckCircle2 size={16} />
        Save advanced settings
      </button>
    </section>
  );
}
