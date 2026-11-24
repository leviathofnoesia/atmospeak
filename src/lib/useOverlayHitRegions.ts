import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { hasTauriRuntime } from "./api";

type Region = { x: number; y: number; width: number; height: number };

/** Keep the Windows region aligned with visible controls, including collapsed states. */
export function useOverlayHitRegions() {
  useEffect(() => {
    if (!hasTauriRuntime()) return;
    let frame = 0;
    let sending = false;
    let pending: Region[] | null = null;
    let previous = "";
    let stopped = false;
    const send = async (regions: Region[]) => {
      pending = regions;
      if (sending) return;
      sending = true;
      try {
        while (pending) {
          const next = pending;
          pending = null;
          await invoke("set_overlay_hit_regions", { regions: next });
        }
      } catch (error) {
        previous = "";
        console.error("Could not update floating control hit areas", error);
      } finally { sending = false; }
    };
    const observed = new Set<Element>();
    const resize = new ResizeObserver(() => schedule());
    const measure = () => {
      frame = 0;
      if (stopped) return;
      const current = new Set<Element>();
      const regions: Region[] = [];
      document.querySelectorAll<HTMLElement>(".dock, .dock-recovery button, .dock-menu button, .dock-alert, .dock-tip--warn").forEach(element => {
        current.add(element);
        if (!observed.has(element)) { observed.add(element); resize.observe(element); }
        if (getComputedStyle(element).visibility === "hidden" || !element.getClientRects().length) return;
        const { x, y, width, height } = element.getBoundingClientRect();
        if (width && height) regions.push({ x, y, width, height });
      });
      for (const element of observed) { if (!current.has(element)) { resize.unobserve(element); observed.delete(element); } }
      const serialized = JSON.stringify(regions);
      if (serialized !== previous) { previous = serialized; void send(regions); }
    };
    function schedule() { if (!stopped && !frame) frame = requestAnimationFrame(measure); }
    const mutations = new MutationObserver(schedule);
    mutations.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "style", "data-state", "data-revealed"] });
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      stopped = true;
      cancelAnimationFrame(frame);
      mutations.disconnect(); resize.disconnect();
      window.removeEventListener("resize", schedule);
      void send([]);
    };
  }, []);
}
