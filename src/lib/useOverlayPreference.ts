import { useCallback, useEffect, useState } from "react";

export type OverlayVisibilityMode = "always" | "recording";

export const OVERLAY_VISIBILITY_STORAGE_KEY = "atmospeak.overlay-visibility";

const OVERLAY_VISIBILITY_EVENT = "atmospeak:overlay-visibility";

function isOverlayVisibilityMode(value: unknown): value is OverlayVisibilityMode {
  return value === "always" || value === "recording";
}

export function readOverlayVisibilityMode(): OverlayVisibilityMode {
  if (typeof window === "undefined") return "always";

  try {
    const value = window.localStorage.getItem(OVERLAY_VISIBILITY_STORAGE_KEY);
    return isOverlayVisibilityMode(value) ? value : "always";
  } catch {
    return "always";
  }
}

export function writeOverlayVisibilityMode(mode: OverlayVisibilityMode) {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(OVERLAY_VISIBILITY_STORAGE_KEY, mode);
  } catch {
    // A restricted WebView may deny storage. The in-memory state still applies.
  }

  window.dispatchEvent(new CustomEvent<OverlayVisibilityMode>(OVERLAY_VISIBILITY_EVENT, { detail: mode }));
}

/**
 * Keeps the overlay visibility choice shared by the settings window and the
 * separate transparent overlay WebView. The default keeps the existing behavior.
 */
export function useOverlayVisibility() {
  const [mode, setMode] = useState<OverlayVisibilityMode>(readOverlayVisibilityMode);

  useEffect(() => {
    const apply = (next: OverlayVisibilityMode) => setMode(next);
    const onStorage = (event: StorageEvent) => {
      if (event.key !== null && event.key !== OVERLAY_VISIBILITY_STORAGE_KEY) return;
      apply(isOverlayVisibilityMode(event.newValue) ? event.newValue : "always");
    };
    const onCustom = (event: Event) => {
      const next = (event as CustomEvent<unknown>).detail;
      if (isOverlayVisibilityMode(next)) apply(next);
    };

    apply(readOverlayVisibilityMode());
    window.addEventListener("storage", onStorage);
    window.addEventListener(OVERLAY_VISIBILITY_EVENT, onCustom);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener(OVERLAY_VISIBILITY_EVENT, onCustom);
    };
  }, []);

  const update = useCallback((next: OverlayVisibilityMode) => {
    setMode(next);
    writeOverlayVisibilityMode(next);
  }, []);

  const setOnlyWhileRecording = useCallback(
    (onlyWhileRecording: boolean) => update(onlyWhileRecording ? "recording" : "always"),
    [update],
  );

  return {
    mode,
    onlyWhileRecording: mode === "recording",
    setMode: update,
    setOnlyWhileRecording,
  };
}
