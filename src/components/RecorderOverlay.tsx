import { Mic, Square, X } from "lucide-react";
import type { ModelStatus, RecordingStarted } from "../types/dictation";
import { StatusLed } from "./StatusLed";

interface RecorderOverlayProps {
  recording: RecordingStarted | null;
  elapsedSeconds: number;
  busy: boolean;
  modelStatus: ModelStatus | null;
  onToggle: () => void;
  onCancel: () => void;
}

export function RecorderOverlay({
  recording,
  elapsedSeconds,
  busy,
  modelStatus,
  onToggle,
  onCancel,
}: RecorderOverlayProps) {
  const isRecording = recording !== null;
  const primaryLabel = isRecording ? "Stop" : "Dictate";
  const timer = `${Math.floor(elapsedSeconds / 60)
    .toString()
    .padStart(2, "0")}:${Math.floor(elapsedSeconds % 60)
    .toString()
    .padStart(2, "0")}`;

  return (
    <aside className="recorder" aria-label="Recorder controls">
      <div className="recorder__meter" aria-hidden="true">
        {Array.from({ length: 18 }, (_, index) => (
          <span key={index} />
        ))}
      </div>
      <div className="recorder__main">
        <div>
          <p className="eyebrow">Global capture</p>
          <h2>{isRecording ? timer : "Ready"}</h2>
          <StatusLed
            tone={isRecording ? "hot" : modelStatus?.ready ? "good" : "warn"}
            label={
              isRecording
                ? recording.microphoneName
                : modelStatus?.ready
                  ? "Bundled engine armed"
                  : "Runtime unavailable"
            }
          />
        </div>
        <div className="recorder__actions">
          <button
            className="button button--primary button--square"
            type="button"
            onClick={onToggle}
            disabled={busy}
            aria-label={primaryLabel}
            title={primaryLabel}
          >
            {isRecording ? <Square size={22} /> : <Mic size={22} />}
          </button>
          <button
            className="button button--ghost button--square"
            type="button"
            onClick={onCancel}
            disabled={!isRecording || busy}
            aria-label="Cancel"
            title="Cancel"
          >
            <X size={20} />
          </button>
        </div>
      </div>
      <div className="recorder__stripe">
        <span>CTRL</span>
        <span>WIN</span>
        <span>SPACE</span>
      </div>
    </aside>
  );
}
