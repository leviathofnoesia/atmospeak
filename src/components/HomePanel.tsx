import { Copy } from "lucide-react";
import type { AppSnapshot, TranscriptSession } from "../types/dictation";
import { Aura } from "./Aura";

interface HomePanelProps {
  snapshot: AppSnapshot;
  recentSession: TranscriptSession | null;
  onCopyRecent: (session: TranscriptSession) => Promise<void>;
}

export function HomePanel({
  snapshot,
  recentSession,
  onCopyRecent,
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

      {recentSession ? (
        <div className="hub__section" style={{ paddingTop: 4 }}>
          <div className="kick" style={{ color: "rgba(27,26,29,0.45)", marginBottom: 12 }}>
            Latest transcript
          </div>
          <div className="latest-transcript">
            <p>{recentSession.cleanedText}</p>
            <button
              className="pill-btn ghost"
              type="button"
              onClick={() => void onCopyRecent(recentSession)}
            >
              <Copy size={14} /> Copy
            </button>
          </div>
        </div>
      ) : (
        <div className="hub__section" style={{ paddingTop: 4 }}>
          <p className="muted">No sessions yet. Hold your shortcut over Notepad to begin.</p>
        </div>
      )}
      <div className="hub__section home-facts">
        <span>Model · {snapshot.settings.activeModelId}</span>
        <span>Privacy · on device</span>
        <span>
          Retention · {snapshot.settings.transcriptRetentionDays === 0
            ? "until deleted"
            : `${snapshot.settings.transcriptRetentionDays} days`}
        </span>
      </div>
    </div>
  );
}
