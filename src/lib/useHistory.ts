import { useEffect, useRef, useState } from "react";
import { getHistorySession, hasTauriRuntime, searchHistory } from "./api";
import type { TranscriptSession } from "../types/dictation";

export function useHistory(seed: TranscriptSession[], query: string, selectedId?: string | null) {
  const [sessions, setSessions] = useState(seed);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const failedPage = useRef<"first" | "more">("first");
  const revision = useRef(0);
  const offset = useRef(0);
  const scope = useRef({ query, selectedId });
  const native = hasTauriRuntime();
  useEffect(() => {
    const request = ++revision.current;
    const changed = scope.current.query !== query || scope.current.selectedId !== selectedId;
    const limit = changed ? 100 : Math.max(100, offset.current);
    scope.current = { query, selectedId };
    if (changed) { offset.current = 0; setHasMore(false); }
    setError("");
    if (!native) {
      const needle = query.trim().toLowerCase();
      setSessions(seed.filter((session) => `${session.rawText} ${session.cleanedText} ${session.polishedText ?? ""}`.toLowerCase().includes(needle)));
      return;
    }
    setLoading(true);
    void (async () => {
      try {
        let page: TranscriptSession[] = [];
        let more = false;
        do {
          const rows = await searchHistory(query, page.length);
          if (request !== revision.current) return;
          page = [...page, ...rows.slice(0, 100)];
          more = rows.length > 100;
        } while (more && page.length < limit);
        const loaded = page.length;
        if (selectedId && !query.trim() && !page.some((session) => session.id === selectedId)) {
          const selected = await getHistorySession(selectedId);
          if (selected) page = [selected, ...page];
          else if (request === revision.current) setError("This recording is no longer in History.");
        }
        if (request !== revision.current) return;
        offset.current = loaded;
        setSessions(page);
        setHasMore(more);
      } catch (cause) {
        if (request === revision.current) { failedPage.current = "first"; setError(String(cause)); }
      } finally {
        if (request === revision.current) setLoading(false);
      }
    })();
    return () => { revision.current += 1; };
  }, [seed, query, selectedId, native, attempt]);

  const loadMore = async () => {
    if (loading || !hasMore) return;
    const request = revision.current;
    setLoading(true);
    setError("");
    try {
      const rows = await searchHistory(query, offset.current);
      if (request !== revision.current) return;
      offset.current += Math.min(rows.length, 100);
      setSessions((current) => [...current, ...rows.slice(0, 100).filter((row) => !current.some((item) => item.id === row.id))]);
      setHasMore(rows.length > 100);
    } catch (cause) {
      if (request === revision.current) { failedPage.current = "more"; setError(String(cause)); }
    } finally {
      if (request === revision.current) setLoading(false);
    }
  };
  const retry = () => {
    if (loading) return;
    if (failedPage.current === "more") void loadMore();
    else setAttempt((current) => current + 1);
  };
  return { sessions, hasMore, loading, error, loadMore, retry };
}
