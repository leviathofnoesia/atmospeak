import { Mic, Square, X } from "lucide-react";
import { memo } from "react";
import type { ModelStatus, RecordingStarted } from "../types/dictation";
import { StatusLed } from "./StatusLed";

export type RecorderPhase = "idle" | "listening" | "processing" | "pasted" | "error";

interface RecorderOverlayProps {
  recording: RecordingStarted | null;
  elapsedSeconds: number;
  busy: boolean;
  phase?: RecorderPhase;
  modelStatus: ModelStatus | null;
  hotkeyLabel?: string;
  notice?: string;
  inputLevel?: number;
  onToggle: () => void;
  onCancel: () => void;
}

const meterBars = Array.from({ length: 18 }, (_, index) => index);

function RecorderOverlayComponent({
  recording,
  elapsedSeconds,
  busy,
  phase,
  modelStatus,
  hotkeyLabel = "CTRL WIN",
  notice,
  inputLevel = 0,
  onToggle,
  onCancel,
}: RecorderOverlayProps) {
  const isRecording = recording !== null;
  const recorderState = phase ?? getRecorderPhase(isRecording, busy, modelStatus?.ready ?? false);
  const primaryLabel = isRecording ? "Stop" : recorderState === "error" ? "Retry dictation" : "Dictate";
  const timer = `${Math.floor(elapsedSeconds / 60)
    .toString()
    .padStart(2, "0")}:${Math.floor(elapsedSeconds % 60)
    .toString()
    .padStart(2, "0")}`;
  const title = recorderTitle(recorderState, timer);
  const status = recorderStatus(recorderState, recording?.microphoneName, modelStatus?.ready ?? false);

  return (
    <aside
      className="recorder"
      aria-label="Recorder controls"
      data-recorder-state={recorderState}
      data-tauri-drag-region
    >
      <div className="recorder__meter" aria-hidden="true">
        {meterBars.map((index) => (
          <span
            key={index}
            style={{
              transform: `scaleY(${meterScale(index, inputLevel, isRecording || busy)})`,
            }}
          />
        ))}
      </div>
      <div className="recorder__main">
        <div>
          <p className="eyebrow">Global capture</p>
          <h2>{title}</h2>
          <StatusLed tone={status.tone} label={status.label} />
          {notice ? <p className="recorder__notice">{notice}</p> : null}
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
        {hotkeyLabel.split("+").map((part) => (
          <span key={part}>{part.trim()}</span>
        ))}
      </div>
    </aside>
  );
}

export const RecorderOverlay = memo(RecorderOverlayComponent);

function getRecorderPhase(isRecording: boolean, busy: boolean, ready: boolean): RecorderPhase {
  if (isRecording) {
    return "listening";
  }
  if (busy) {
    return "processing";
  }
  return ready ? "idle" : "error";
}

function recorderTitle(phase: RecorderPhase, timer: string) {
  switch (phase) {
    case "listening":
      return timer;
    case "processing":
      return "Processing";
    case "pasted":
      return "Pasted";
    case "error":
      return "Check input";
    case "idle":
      return "Ready";
  }
}

function recorderStatus(
  phase: RecorderPhase,
  microphoneName: string | undefined,
  ready: boolean,
): { tone: "idle" | "good" | "warn" | "hot"; label: string } {
  switch (phase) {
    case "listening":
      return { tone: "hot", label: microphoneName ?? "Listening" };
    case "processing":
      return { tone: "warn", label: "Transcribing locally" };
    case "pasted":
      return { tone: "good", label: "Transcript delivered" };
    case "error":
      return { tone: "warn", label: ready ? "Needs attention" : "Runtime unavailable" };
    case "idle":
      return { tone: ready ? "good" : "warn", label: ready ? "Bundled engine armed" : "Runtime unavailable" };
  }
}

function meterScale(index: number, level: number, active: boolean) {
  if (!active) {
    return 0.18 + ((index % 5) * 0.035);
  }

  const normalized = Math.max(0, Math.min(1, level));
  const contour = 0.45 + Math.sin(index * 0.9) * 0.22 + Math.cos(index * 0.42) * 0.16;
  return Math.max(0.12, Math.min(1, 0.14 + normalized * contour));
}
