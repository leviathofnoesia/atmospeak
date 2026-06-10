import {
  Clipboard,
  Copy,
  Download,
  History,
  Sparkles,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  searchSessions,
} from "../lib/api";
import type {
  ExportFormat,
  HistorySearchFilters,
  RecentAppUsage,
  TranscriptSession,
} from "../types/dictation";
import { EmptyState } from "./EmptyState";
import { PanelTitle } from "./PanelTitle";

interface HistoryPanelProps {
  sessions: TranscriptSession[];
  onCopy: (session: TranscriptSession) => Promise<void>;
  onInject: (session: TranscriptSession) => Promise<void>;
  onPolish: (session: TranscriptSession) => Promise<void>;
  polishingSessionId: string | null;
  onExport: (session: TranscriptSession, format: ExportFormat) => Promise<void>;
  onUpdateNotes: (session: TranscriptSession, notes: string) => Promise<void>;
  recentApps: RecentAppUsage[];
}

function parseOptionalWordCount(value: string) {
  const trimmed = value.trim();
  if (trimmed.length === 0) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

function stringifyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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

export function HistoryPanel({
  sessions,
  onCopy,
  onInject,
  onPolish,
  polishingSessionId,
  onExport,
  onUpdateNotes,
  recentApps,
}: HistoryPanelProps) {
  const [query, setQuery] = useState("");
  const [fromDate, setFromDate] = useState("");
  const [toDate, setToDate] = useState("");
  const [minWords, setMinWords] = useState("");
  const [maxWords, setMaxWords] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [draftNotes, setDraftNotes] = useState<Record<string, string>>({});
  const [filtered, setFiltered] = useState<TranscriptSession[]>(sessions);
  const [filterError, setFilterError] = useState<string | null>(null);

  const searchFilters = useMemo<HistorySearchFilters>(
    () => ({
      query: query.trim() || null,
      fromDate: fromDate || null,
      toDate: toDate || null,
      minWordCount: parseOptionalWordCount(minWords),
      maxWordCount: parseOptionalWordCount(maxWords),
      limit: 200,
    }),
    [fromDate, maxWords, minWords, query, toDate],
  );

  useEffect(() => {
    let cancelled = false;
    const runSearch = async () => {
      const hasFilters =
        searchFilters.query !== null ||
        searchFilters.fromDate !== null ||
        searchFilters.toDate !== null ||
        searchFilters.minWordCount !== null ||
        searchFilters.maxWordCount !== null;
      if (!hasFilters) {
        setFiltered(sessions);
        setFilterError(null);
        return;
      }

      try {
        const nextSessions = await searchSessions(searchFilters);
        if (!cancelled) {
          setFiltered(nextSessions);
          setFilterError(null);
        }
      } catch (error) {
        if (!cancelled) {
          setFiltered(sessions);
          setFilterError(stringifyError(error));
        }
      }
    };

    const timer = window.setTimeout(() => {
      void runSearch();
    }, 160);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [searchFilters, sessions]);

  return (
    <section className="list-panel">
      <PanelTitle icon={<History size={22} />} title="Transcript history" />
      <div className="history-toolbar">
        <input
          type="search"
          placeholder="Search transcripts, apps, notes"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <input
          type="date"
          aria-label="From date"
          value={fromDate}
          onChange={(event) => setFromDate(event.currentTarget.value)}
        />
        <input
          type="date"
          aria-label="To date"
          value={toDate}
          onChange={(event) => setToDate(event.currentTarget.value)}
        />
        <input
          type="number"
          min="0"
          inputMode="numeric"
          placeholder="Min words"
          value={minWords}
          onChange={(event) => setMinWords(event.currentTarget.value)}
        />
        <input
          type="number"
          min="0"
          inputMode="numeric"
          placeholder="Max words"
          value={maxWords}
          onChange={(event) => setMaxWords(event.currentTarget.value)}
        />
      </div>
      {filterError ? <p className="history-filter-error">Filter unavailable: {filterError}</p> : null}
      {recentApps.length > 0 ? (
        <div className="recent-apps">
          <span className="eyebrow">Top apps</span>
          <div className="recent-apps__row">
            {recentApps.map((app) => (
              <span className="chip" key={app.name} title={`${app.sessionCount} sessions`}>
                {app.name} <small>· {app.category}</small>
              </span>
            ))}
          </div>
        </div>
      ) : null}
      {filtered.length === 0 ? (
        <EmptyState
          text={
            sessions.length === 0
              ? "No transcripts yet. Start a recording from the floating control."
              : "No transcripts match the current search."
          }
        />
      ) : (
        filtered.map((session) => {
          const expanded = expandedId === session.id;
          const minutes = session.durationMs / 60_000;
          const wpm = minutes > 0 ? Math.round(session.wordCount / minutes) : 0;
          return (
            <article
              className={`history-item ${expanded ? "is-expanded" : ""}`}
              key={session.id}
            >
              <div className="history-item__row">
                <button
                  type="button"
                  className="history-item__summary"
                  onClick={() => setExpandedId(expanded ? null : session.id)}
                  aria-expanded={expanded}
                >
                  <span className="history-item__date">{formatDate(session.createdAt)}</span>
                  <p>{session.cleanedText}</p>
                  <small>
                    {session.wordCount} words · {formatDuration(session.durationMs)} · {wpm} wpm
                    {session.appName ? ` · ${session.appName}` : ""}
                  </small>
                </button>
                <div className="history-item__actions">
                  <button
                    className="button button--ghost button--square"
                    type="button"
                    onClick={() => {
                      void onCopy(session);
                    }}
                    aria-label="Copy transcript"
                    title="Copy transcript"
                  >
                    <Copy size={18} />
                  </button>
                  <button
                    className="button button--ghost button--square"
                    type="button"
                    onClick={() => {
                      void onInject(session);
                    }}
                    aria-label="Paste transcript again"
                    title="Paste transcript again"
                  >
                    <Clipboard size={18} />
                  </button>
                  <button
                    className="button button--ghost button--square"
                    type="button"
                    onClick={() => {
                      void onPolish(session);
                    }}
                    disabled={polishingSessionId === session.id}
                    aria-label="AI edit transcript"
                    title="AI edit transcript"
                  >
                    <Sparkles size={18} />
                  </button>
                </div>
              </div>
              {expanded ? (
                <div className="history-item__detail">
                  <div className="history-item__stats">
                    <span>
                      <strong>{session.wordCount}</strong> words
                    </span>
                    <span>
                      <strong>{formatDuration(session.durationMs)}</strong> duration
                    </span>
                    <span>
                      <strong>{wpm}</strong> wpm
                    </span>
                    <span>
                      <strong>{session.appName ?? "Unknown"}</strong> app
                    </span>
                  </div>
                  {session.audioPath ? (
                    <audio
                      controls
                      preload="none"
                      src={
                        session.audioPath.startsWith("http") ||
                        session.audioPath.startsWith("app:") ||
                        session.audioPath.startsWith("mock:")
                          ? session.audioPath
                          : `tauri://localhost/${session.audioPath.replace(/\\/g, "/")}`
                      }
                    />
                  ) : null}
                  <label>
                    <span>Notes</span>
                    <textarea
                      value={draftNotes[session.id] ?? session.notes}
                      rows={2}
                      onChange={(event) =>
                        setDraftNotes({ ...draftNotes, [session.id]: event.currentTarget.value })
                      }
                      onBlur={() => {
                        const value = draftNotes[session.id] ?? session.notes;
                        if (value !== session.notes) {
                          void onUpdateNotes(session, value);
                        }
                      }}
                    />
                  </label>
                  <div className="history-item__exports">
                    {(["txt", "md", "json", "srt"] as ExportFormat[]).map((format) => (
                      <button
                        key={format}
                        className="button button--ghost"
                        type="button"
                        onClick={() => void onExport(session, format)}
                      >
                        <Download size={14} /> {format.toUpperCase()}
                      </button>
                    ))}
                  </div>
                </div>
              ) : null}
            </article>
          );
        })
      )}
    </section>
  );
}
