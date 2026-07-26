import { Mic } from "lucide-react";
import type { AppSnapshot, ModelStatus, StageMetrics, TranscriptSession } from "../types/dictation";
import { Aura } from "./Aura";

interface HomePanelProps {
  snapshot: AppSnapshot;
  modelStatus: ModelStatus | null;
  recentSession: TranscriptSession | null;
  lastMetrics: StageMetrics | null;
  onStart: () => void;
  busy: boolean;
}

export function HomePanel({
  snapshot,
  modelStatus,
  recentSession,
  lastMetrics,
  onStart,
  busy,
}: HomePanelProps) {
  const { stats } = snapshot;
  return (
    <div>
      <div className="hub__head">
        <div className="kick">P.01 / Home — the room at rest</div>
        <h1>
          Hold, speak, <em>release.</em>
        </h1>
      </div>

      <div className="hub__section">
        <div className="hub-hero">
          <div>
            <h2>
              Atmospeak listens on device, cleans the words, and <em>sets them down</em> wherever
              your cursor rests.
            </h2>
            <p>
              Nothing leaves your machine. The companion waits at the edge of the screen until you
              call it.
            </p>
            <div style={{ marginTop: 20 }}>
              <button className="pill-btn accent" type="button" onClick={onStart} disabled={busy}>
                <Mic size={15} />
                Start dictation
              </button>
            </div>
          </div>
          <div className="hub-hero__aura">
            <Aura size={124} active />
          </div>
        </div>
      </div>

      <div className="hub__section" style={{ paddingTop: 0 }}>
        <div className="stat-row">
          <div className="stat">
            <div className="k">Sessions</div>
            <div className="v">{stats.totalSessions}</div>
          </div>
          <div className="stat">
            <div className="k">Words dictated</div>
            <div className="v accent">{stats.totalWords.toLocaleString()}</div>
          </div>
          <div className="stat">
            <div className="k">Avg · words/min</div>
            <div className="v">{Math.round(stats.averageWordsPerMinute)}</div>
          </div>
        </div>
      </div>

      <div className="hub__section" style={{ paddingTop: 4 }}>
        <div className="kick" style={{ color: "rgba(27,26,29,0.45)", marginBottom: 12 }}>
          Runtime
        </div>
        <p style={{ fontSize: 14, color: "#2a2930" }}>
          {modelStatus?.message ?? "Checking transcription runtime…"}
        </p>
        {lastMetrics ? (
          <p className="muted" style={{ marginTop: 6 }}>
            Last pipeline: {lastMetrics.totalMs}ms total · ASR {lastMetrics.asrMs}ms (
            {lastMetrics.asrBackend})
          </p>
        ) : null}
      </div>

      {recentSession ? (
        <div className="hub__section" style={{ paddingTop: 4 }}>
          <div className="kick" style={{ color: "rgba(27,26,29,0.45)", marginBottom: 12 }}>
            Latest transcript
          </div>
          <p style={{ fontSize: 16, lineHeight: 1.6, color: "#2a2930" }}>
            {recentSession.cleanedText}
          </p>
        </div>
      ) : (
        <div className="hub__section" style={{ paddingTop: 4 }}>
          <p className="muted">No sessions yet. Hold your shortcut over Notepad to begin.</p>
        </div>
      )}
    </div>
  );
}
