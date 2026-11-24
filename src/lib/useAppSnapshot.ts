import { useCallback, useEffect, useRef, useState } from "react";
import { getAppSnapshot } from "./api";
import type { AppSnapshot } from "../types/dictation";

/** Background refreshes cannot replace a newer mutation or refresh. */
export function useAppSnapshot(load = getAppSnapshot) {
  const [snapshot, updateSnapshot] = useState<AppSnapshot | null>(null);
  const revision = useRef(0);
  const current = useRef<AppSnapshot | null>(null);

  const setSnapshot = useCallback((next: AppSnapshot) => {
    revision.current += 1;
    current.current = next;
    updateSnapshot(next);
  }, []);

  const refreshSnapshot = useCallback(async () => {
    const request = ++revision.current;
    const next = await load();
    if (request === revision.current) {
      current.current = next;
      updateSnapshot(next);
    }
    return current.current ?? next;
  }, [load]);

  useEffect(() => () => { revision.current += 1; }, []);
  return { snapshot, setSnapshot, refreshSnapshot };
}
