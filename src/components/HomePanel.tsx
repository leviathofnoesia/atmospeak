import { Database, Mic } from "lucide-react";
import type { AppSnapshot, ModelStatus, StageMetrics, TranscriptSession } from "../types/dictation";
import { Aura } from "./Aura";
import { MetricCard } from "./MetricCard";

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
  return (
    <section className="panel-grid">
      <div className="hero-panel">
        <div className="hero-panel__body">
          <p className="eyebrow">Working mode</p>
          <h2>Hold, speak, release. Atmospeak cleans the words and pastes them locally.</h2>
          <p className="muted">
            Transcription uses the bundled whisper.cpp CLI. Short phrases often take a few seconds
            after release — reliability first, not “instant cloud” latency.
          </p>
          <button className="button button--primary" type="button" onClick={onStart} disabled={busy}>
            <Mic size={18} />
            Start dictation
          </button>
        </div>
        <div className="hero-panel__aura">
          <Aura size={116} active />
        </div>
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
          <h3>Runtime</h3>
        </div>
        <p>{modelStatus?.message ?? "Checking transcription runtime…"}</p>
        {lastMetrics ? (
          <p className="muted">
            Last pipeline: {lastMetrics.totalMs}ms total · ASR {lastMetrics.asrMs}ms (
            {lastMetrics.asrBackend})
          </p>
        ) : null}
        {recentSession ? (
          <p className="muted">Latest: “{recentSession.cleanedText.slice(0, 120)}”</p>
        ) : (
          <p className="muted">No sessions yet. Hold your shortcut over Notepad to begin.</p>
        )}
      </div>
    </section>
  );
}
