import { Cpu, Database, Mic, Sparkles } from "lucide-react";
import type { AppSnapshot, ModelStatus, TranscriptSession } from "../types/dictation";
import { Aura } from "./Aura";
import { MetricCard } from "./MetricCard";
import { ToggleRow } from "./ToggleRow";

interface HomePanelProps {
  snapshot: AppSnapshot;
  modelStatus: ModelStatus | null;
  recentSession: TranscriptSession | null;
  onStart: () => void;
  onPolishLatest: (session: TranscriptSession) => Promise<void>;
  polishingSessionId: string | null;
  onUpdatePrivacy: (
    privacyMode: boolean,
    autoDeleteTranscriptsAfterMinutes: number | null,
  ) => Promise<void>;
  busy: boolean;
}

export function HomePanel({
  snapshot,
  modelStatus,
  recentSession,
  onStart,
  onPolishLatest,
  polishingSessionId,
  onUpdatePrivacy,
  busy,
}: HomePanelProps) {
  return (
    <section className="panel-grid">
      <div className="hero-panel">
        <div className="hero-panel__body">
          <p className="eyebrow">Working mode</p>
          <h2>Hold, speak, release. Atmospeak cleans the words and sets them down locally.</h2>
          <button className="button button--primary" type="button" onClick={onStart} disabled={busy}>
            <Mic size={18} />
            Start dictation
          </button>
        </div>
        <div className="hero-panel__aura"><Aura size={116} active /></div>
      </div>
      <MetricCard label="Sessions" value={snapshot.stats.totalSessions.toString()} />
      <MetricCard label="Words" value={snapshot.stats.totalWords.toString()} />
      <MetricCard
        label="WPM"
        value={Math.round(snapshot.stats.averageWordsPerMinute).toString()}
      />
      <div className="machine-card">
        <div className="machine-card__header">
          <Database size={20} />
          <h3>Privacy</h3>
        </div>
        <ToggleRow
          icon={<Database size={18} />}
          label="Privacy mode"
          checked={snapshot.settings.privacyMode}
          onChange={(privacyMode) =>
            void onUpdatePrivacy(
              privacyMode,
              privacyMode
                ? (snapshot.settings.autoDeleteTranscriptsAfterMinutes ?? 1440)
                : snapshot.settings.autoDeleteTranscriptsAfterMinutes,
            )
          }
        />
        <label>
          <span>Auto-delete transcripts</span>
          <select
            value={snapshot.settings.autoDeleteTranscriptsAfterMinutes ?? ""}
            onChange={(event) =>
              void onUpdatePrivacy(
                snapshot.settings.privacyMode,
                event.currentTarget.value.length > 0 ? Number(event.currentTarget.value) : null,
              )
            }
          >
            <option value="">Never</option>
            <option value={15}>After 15 minutes</option>
            <option value={60}>After 1 hour</option>
            <option value={1440}>After 1 day</option>
            <option value={10080}>After 7 days</option>
          </select>
        </label>
      </div>
      <div className="machine-card">
        <div className="machine-card__header">
          <Cpu size={20} />
          <h3>Offline engine</h3>
        </div>
        <p>{modelStatus?.message ?? "Checking model status..."}</p>
      </div>
      <div className="machine-card machine-card--wide">
        <div className="machine-card__header">
          <Database size={20} />
          <h3>Latest transcript</h3>
        </div>
        <p>{recentSession?.cleanedText ?? "History is empty. Your first transcript will appear here."}</p>
        {recentSession ? (
          <button
            className="button button--ghost"
            type="button"
            onClick={() => void onPolishLatest(recentSession)}
            disabled={polishingSessionId === recentSession.id}
          >
            <Sparkles size={18} />
            {polishingSessionId === recentSession.id ? "Editing" : "Edit last"}
          </button>
        ) : null}
      </div>
    </section>
  );
}
