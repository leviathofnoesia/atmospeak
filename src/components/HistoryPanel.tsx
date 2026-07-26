import { Clipboard, Copy, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { TranscriptSession } from "../types/dictation";
import { EmptyState } from "./EmptyState";

interface HistoryPanelProps {
  sessions: TranscriptSession[];
  onCopy: (session: TranscriptSession) => Promise<void>;
  onInject: (session: TranscriptSession) => Promise<void>;
  onDelete: (session: TranscriptSession) => Promise<void>;
}

function formatDuration(durationMs: number) {
  const seconds = Math.round(durationMs / 1000);
  return `${seconds}s`;
}

function formatTime(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

export function HistoryPanel({ sessions, onCopy, onInject, onDelete }: HistoryPanelProps) {
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((session) => session.cleanedText.toLowerCase().includes(q));
  }, [query, sessions]);

  return (
    <div>
      <div className="hub__head">
        <div className="kick">P.02 / History — everything you&rsquo;ve said</div>
        <h1>
          Said &amp; <em>set down.</em>
        </h1>
      </div>

      <div className="hub__section" style={{ paddingTop: 18 }}>
        <label className="history-filter">
          <span className="kick">Filter</span>
          <input
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            placeholder="Search transcripts…"
          />
        </label>

        {filtered.length === 0 ? (
          <EmptyState text="No transcripts yet. Dictations appear here after a successful session." />
        ) : (
          <div className="tx-list">
            {filtered.map((session, index) => {
              const expanded = expandedId === session.id;
              return (
                <div className="tx" key={session.id}>
                  <div className="tx__idx">
                    {String(filtered.length - index).padStart(2, "0")}
                  </div>
                  <div className="tx__txt">
                    <span className="app">{session.sourceApplication ?? "Local dictation"}</span>
                    <button
                      type="button"
                      className="tx__open"
                      onClick={() => setExpandedId(expanded ? null : session.id)}
                      aria-expanded={expanded}
                    >
                      {expanded ? session.cleanedText : session.cleanedText.slice(0, 140)}
                    </button>
                    {expanded ? (
                      <div className="tx__detail">
                        {session.audioPath && !session.audioPath.startsWith("mock") ? (
                          <audio controls src={session.audioPath}>
                            <track kind="captions" />
                          </audio>
                        ) : null}
                        <div className="tx__actions">
                          <button
                            type="button"
                            className="pill-btn ghost"
                            onClick={() => void onCopy(session)}
                          >
                            <Copy size={14} />
                            Copy
                          </button>
                          <button
                            type="button"
                            className="pill-btn ghost"
                            onClick={() => void onInject(session)}
                          >
                            <Clipboard size={14} />
                            Paste again
                          </button>
                          <button
                            type="button"
                            className="pill-btn ghost"
                            onClick={() => void onDelete(session)}
                          >
                            <Trash2 size={14} />
                            Delete
                          </button>
                        </div>
                      </div>
                    ) : null}
                  </div>
                  <div className="tx__meta">
                    {formatTime(session.createdAt)}
                    <br />
                    {session.wordCount} w · {formatDuration(session.durationMs)}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
