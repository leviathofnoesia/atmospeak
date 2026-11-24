import { listen } from "@tauri-apps/api/event";
import { getDictationSnapshot } from "./api";
import type { NativeDictationEvent } from "../types/dictation";

export type DictationRecovery = { retry: () => Promise<void> } | null;

/** Subscribe before hydration and keep live updates connected through request failures. */
export async function watchDictation(
  accept: (event: NativeDictationEvent) => void,
  recovery: (state: DictationRecovery) => void = () => {},
) {
  let generation = -1;
  let active = true;
  let loading = false;
  const receive = (event: NativeDictationEvent) => {
    if (!active || event.generation <= generation) return;
    generation = event.generation;
    recovery(null);
    accept(event);
  };
  const unlisten = await listen<NativeDictationEvent>("wind-speak://native-dictation", ({ payload }) => receive(payload));
  const hydrate = async () => {
    if (!active || loading) return;
    loading = true;
    const startedAt = generation;
    try {
      const snapshot = await getDictationSnapshot();
      if (!active) return;
      if (snapshot) receive(snapshot);
      recovery(null);
    } catch {
      if (active && generation === startedAt) recovery({ retry: hydrate });
    } finally {
      loading = false;
    }
  };
  void hydrate();
  return () => { active = false; unlisten(); };
}
