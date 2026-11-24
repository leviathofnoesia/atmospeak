import { Cpu } from "lucide-react";
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
}

export function AdvancedPanel({
  settings,
  setSettings,
  modelStatus,
  lastMetrics,
  runtimeEvents,
  onRunDiagnosticSoundCheck,
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
          Atmospeak ships with a resident CPU transcription engine and a bundled English model.
          The engine keeps the selected model warm. Override paths only for custom builds.
        </p>
      </div>
      {!import.meta.env.PROD ? (
        <>
          <ToggleRow
            icon={<Cpu size={18} />}
            label="Use advanced runtime override"
            checked={settings.advancedRuntimeEnabled}
            onChange={(advancedRuntimeEnabled) =>
              setSettings({ ...settings, advancedRuntimeEnabled })
            }
          />
          <label>
            <span>Advanced ASR executable path</span>
            <input
              value={settings.advancedAsrExecutablePath}
              disabled={!settings.advancedRuntimeEnabled}
              onChange={(event) =>
                setSettings({ ...settings, advancedAsrExecutablePath: event.currentTarget.value })
              }
              placeholder="C:\\path\\to\\atmospeak-asr-cpu.exe"
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
              placeholder="C:\\path\\to\\model.bin"
            />
          </label>
        </>
      ) : (
        <p className="muted">
          Executable runtime overrides are disabled in production builds.
        </p>
      )}
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
        <p>ASR executable · {modelStatus?.executablePath || "unavailable"}</p>
        <p>Model · {modelStatus?.modelPath || "unavailable"}</p>
      </div>
      <PanelTitle icon={<Cpu size={22} />} title="Runtime logs" />
      <RuntimeEventList events={runtimeEvents} />
      <button type="button" className="button button--ghost" onClick={() => void onRunDiagnosticSoundCheck()}>
        Run diagnostic sound check
      </button>
    </section>
  );
}
