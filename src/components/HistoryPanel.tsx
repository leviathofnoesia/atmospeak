import { Clipboard, Copy, History } from "lucide-react";
import { useMemo, useState } from "react";
import type { TranscriptSession } from "../types/dictation";
import { EmptyState } from "./EmptyState";
import { PanelTitle } from "./PanelTitle";

interface HistoryPanelProps {
  sessions: TranscriptSession[];
  onCopy: (session: TranscriptSession) => Promise<void>;
  onInject: (session: TranscriptSession) => Promise<void>;
}

function formatDuration(durationMs: number) {
  const seconds = Math.round(durationMs / 1000);
  return `${seconds}s`;
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

export function HistoryPanel({ sessions, onCopy, onInject }: HistoryPanelProps) {
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((session) => session.cleanedText.toLowerCase().includes(q));
  }, [query, sessions]);

  return (
    <section className="history-panel">
      <PanelTitle icon={<History size={22} />} title="Transcript history" />
      <label>
        <span>Filter</span>
        <input
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
          placeholder="Search transcripts…"
        />
      </label>
      {filtered.length === 0 ? (
        <EmptyState text="No transcripts yet. Dictations appear here after a successful session." />
      ) : (
        <ul className="history-list">
          {filtered.map((session) => {
            const expanded = expandedId === session.id;
            return (
              <li key={session.id} className="history-row">
                <button
                  type="button"
                  className="history-row__main"
                  onClick={() => setExpandedId(expanded ? null : session.id)}
                >
                  <strong>{session.cleanedText.slice(0, 140)}</strong>
                  <span>
                    {formatDate(session.createdAt)} · {session.wordCount} words ·{" "}
                    {formatDuration(session.durationMs)}
                  </span>
                </button>
                <div className="history-row__actions">
                  <button
                    type="button"
                    className="button button--ghost"
                    aria-label="Copy transcript"
                    onClick={() => void onCopy(session)}
                  >
                    <Copy size={16} />
                    Copy
                  </button>
                  <button
                    type="button"
                    className="button button--ghost"
                    aria-label="Paste transcript again"
                    onClick={() => void onInject(session)}
                  >
                    <Clipboard size={16} />
                    Paste
                  </button>
                </div>
                {expanded ? (
                  <div className="history-row__detail">
                    <p>{session.cleanedText}</p>
                    {session.audioPath ? (
                      <audio controls src={session.audioPath.startsWith("mock") ? undefined : session.audioPath}>
                        <track kind="captions" />
                      </audio>
                    ) : null}
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
