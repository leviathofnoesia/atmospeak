import clsx from "clsx";
import { memo, useEffect, useRef } from "react";
import type { CSSProperties } from "react";
import type { DictationMode, ModelStatus, RecordingStarted } from "../types/dictation";
import { Aura } from "./Aura";
import "./RecorderOverlay.css";

export type RecorderPhase =
  | "idle"
  | "listening"
  | "finalizing"
  | "pasted"
  | "saved"
  | "error";
export type RecorderAccent = "dusk" | "teal" | "lilac";
export type RecorderTheme = "dark" | "light";
export type RecorderWaveStyle = "ribbon" | "bars" | "pulse";
export type RecorderDockShape = "orb" | "capsule" | "tape";
export type RecorderMotion = "lively" | "calm";
export type BubbleSize = "small" | "medium" | "large";

// Kept for back-compat with callers that still import these names.
interface LiveTranscript {
  sessionId: string | null;
  phase: "idle" | "partial" | "stable" | "final" | "error";
  text?: string;
  stableText?: string;
  partialText?: string;
  latencyMs: number | null;
}

interface RecorderOverlayProps {
  recording: RecordingStarted | null;
  elapsedSeconds: number;
  busy: boolean;
  phase?: RecorderPhase;
  modelStatus: ModelStatus | null;
  hotkeyLabel?: string;
  shortcutArmed?: boolean;
  mode?: DictationMode;
  notice?: string;
  liveTranscript?: LiveTranscript;
  inputLevel?: number;
  inputBands?: number[];
  bubbleOpacity?: number;
  bubbleSize?: BubbleSize;
  hostApp?: string;
  accent?: RecorderAccent;
  theme?: RecorderTheme;
  waveStyle?: RecorderWaveStyle;
  /**
   * Fired once the press passes the drag threshold, with the originating event so
   * the host can track screen coordinates. The host owns moving the window.
   */
  onMoveStart?: (event: React.PointerEvent) => void;
  /** Resting silhouette; the dock always morphs to a capsule while active. */
  dockShape?: RecorderDockShape;
  /** Animation tempo — `calm` lengthens the breath cycle. */
  motion?: RecorderMotion;
  onToggle: () => void;
  onCancel: () => void;
  onPressStart?: () => void;
  onPressEnd?: () => void;
  onOpenHub?: () => void;
}

const ACCENTS: Record<RecorderAccent, { accent: string; soft: string; deep: string; glow: string; neon: string }> = {
  dusk: { accent: "#485696", soft: "#8a96cf", deep: "#2f3a6e", glow: "rgba(72,86,150,0.45)", neon: "#9db0ff" },
  teal: { accent: "#689689", soft: "#8fc0b3", deep: "#3f6258", glow: "rgba(104,150,137,0.45)", neon: "#74f3cf" },
  lilac: { accent: "#be95c4", soft: "#d8bcdc", deep: "#7a5586", glow: "rgba(190,149,196,0.50)", neon: "#eaa6ff" },
};

const BUBBLE_SCALE: Record<BubbleSize, number> = {
  small: 0.88,
  medium: 1,
  large: 1.08,
};

// ── inline glyphs (match the design's custom marks) ──────────────────
function InsertGlyph({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3v10M8.5 9.5L12 13l3.5-3.5M5 20h14" />
    </svg>
  );
}
function UndoGlyph({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 7L4 12l5 5M4 12h11a5 5 0 0 1 0 10h-1" />
    </svg>
  );
}
function CheckGlyph({ size = 15 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 13l4 4L19 7" />
    </svg>
  );
}

// ── WaveCanvas — live voice visualisation inside the expanded dock ──
function WaveCanvas({
  levelRef,
  style,
  accent,
  accentSoft,
  active,
}: {
  levelRef: React.MutableRefObject<number>;
  style: RecorderWaveStyle;
  accent: string;
  accentSoft: string;
  active: boolean;
}) {
  const ref = useRef<HTMLCanvasElement | null>(null);
  const buf = useRef<number[]>(Array.from({ length: 110 }, () => 0.04));
  const raf = useRef<number | null>(null);

  useEffect(() => {
    if (!active) return undefined;
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    let w = 0;
    let h = 0;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const resize = () => {
      w = cv.clientWidth;
      h = cv.clientHeight;
      cv.width = w * dpr;
      cv.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(resize) : null;
    ro?.observe(cv);

    const rr = (c: CanvasRenderingContext2D, x: number, y: number, ww: number, hh: number, r: number) => {
      c.beginPath();
      c.moveTo(x + r, y);
      c.arcTo(x + ww, y, x + ww, y + hh, r);
      c.arcTo(x + ww, y + hh, x, y + hh, r);
      c.arcTo(x, y + hh, x, y, r);
      c.arcTo(x, y, x + ww, y, r);
      c.closePath();
    };

    let lastFrameAt = 0;
    const draw = (timestamp: number) => {
      raf.current = requestAnimationFrame(draw);
      if (timestamp - lastFrameAt < 16.5) return;
      lastFrameAt = timestamp;
      if (cv.clientWidth !== w || cv.clientHeight !== h) resize();
      if (w === 0 || h === 0) return;
      const arr = buf.current;
      arr.push(active ? levelRef.current : 0.04);
      if (arr.length > 110) arr.shift();
      ctx.clearRect(0, 0, w, h);
      const mid = h / 2;
      const n = arr.length;

      if (style === "bars") {
        const barW = 3;
        const gap = 3;
        const count = Math.floor(w / (barW + gap));
        for (let i = 0; i < count; i++) {
          const v = arr[Math.floor((i / count) * n)] || 0.04;
          const bh = Math.max(2, v * (h - 2));
          const x = i * (barW + gap);
          const g = ctx.createLinearGradient(0, mid - bh / 2, 0, mid + bh / 2);
          g.addColorStop(0, accentSoft);
          g.addColorStop(1, accent);
          ctx.fillStyle = g;
          rr(ctx, x, mid - bh / 2, barW, bh, 1.5);
          ctx.fill();
        }
      } else if (style === "pulse") {
        const count = 30;
        for (let i = 0; i < count; i++) {
          const v = arr[Math.floor((i / count) * n)] || 0.04;
          const x = (i + 0.5) * (w / count);
          const r = 1.4 + v * 5.5;
          ctx.beginPath();
          ctx.arc(x, mid, r, 0, Math.PI * 2);
          ctx.fillStyle = i % 2 ? accent : accentSoft;
          ctx.globalAlpha = 0.35 + v * 0.65;
          ctx.fill();
          ctx.globalAlpha = 1;
        }
      } else {
        // ribbon — calm, low-contrast aurora (two soft layers, easy on the eyes)
        const layers = [
          { amp: 1.0, alpha: 0.46, col: accent, ph: 0 },
          { amp: 0.6, alpha: 0.2, col: accentSoft, ph: 1.7 },
        ];
        const tn = performance.now() / 600;
        layers.forEach((L) => {
          ctx.beginPath();
          for (let i = 0; i <= n; i++) {
            const x = (i / n) * w;
            const v = (arr[i] || 0.04) * L.amp;
            const ripple = Math.sin(i * 0.5 + tn + L.ph) * 0.12 * v;
            const y = mid - (v + ripple) * (h / 2 - 1);
            i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
          }
          for (let i = n; i >= 0; i--) {
            const x = (i / n) * w;
            const v = (arr[i] || 0.04) * L.amp;
            const ripple = Math.sin(i * 0.5 + tn + L.ph) * 0.12 * v;
            ctx.lineTo(x, mid + (v + ripple) * (h / 2 - 1));
          }
          ctx.closePath();
          ctx.fillStyle = L.col;
          ctx.globalAlpha = L.alpha;
          ctx.fill();
          ctx.globalAlpha = 1;
        });
      }
    };
    raf.current = requestAnimationFrame(draw);
    return () => {
      if (raf.current) cancelAnimationFrame(raf.current);
      ro?.disconnect();
    };
  }, [style, accent, accentSoft, active, levelRef]);

  return <canvas ref={ref} className="dock__wave" />;
}

function dockStateFromPhase(
  phase: RecorderPhase,
): "rest" | "listening" | "processing" | "delivered" | "error" {
  switch (phase) {
    case "listening":
      return "listening";
    case "finalizing":
      return "processing";
    case "pasted":
    case "saved":
      return "delivered";
    case "error":
      return "error";
    default:
      return "rest";
  }
}

function RecorderOverlayComponent(props: RecorderOverlayProps) {
  const {
    recording,
    elapsedSeconds,
    busy,
    phase,
    modelStatus,
    liveTranscript,
    inputLevel = 0,
    inputBands,
    bubbleOpacity,
    bubbleSize = "medium",
    hostApp = "your cursor",
    hotkeyLabel = "your shortcut",
    shortcutArmed = false,
    mode = "pushToTalk",
    accent = "dusk",
    theme = "dark",
    waveStyle = "ribbon",
    dockShape = "orb",
    motion = "lively",
    onToggle,
    onCancel,
    onMoveStart,
    onOpenHub,
    notice,
  } = props;

  const isRecording = recording !== null;
  const resolvedPhase: RecorderPhase = phase ?? (isRecording ? "listening" : busy ? "finalizing" : "idle");
  const state = dockStateFromPhase(resolvedPhase);
  const listening = state === "listening";

  const pigment = ACCENTS[accent] ?? ACCENTS.dusk;
  const dockRef = useRef<HTMLDivElement>(null);
  const txRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ sx: number; sy: number; moved: boolean } | null>(null);

  // live amplitude target read by the wave — fed from real input level/bands
  const levelRef = useRef(0.05);
  const peakBand = inputBands && inputBands.length ? Math.max(...inputBands) : 0;
  levelRef.current = Math.max(0.04, Math.min(1, Math.max(inputLevel, peakBand)));

  // keep the latest words in view; only fade the left once text overflows
  const stableTranscript = liveTranscript?.stableText ?? liveTranscript?.text ?? "";
  const partialTranscript = liveTranscript?.partialText ?? "";
  const transcript = `${stableTranscript} ${partialTranscript}`.trim();
  useEffect(() => {
    const el = txRef.current;
    if (!el) return;
    const overflowing = el.scrollWidth > el.clientWidth + 1;
    el.classList.toggle("is-scrolled", overflowing);
    if (overflowing) el.scrollLeft = el.scrollWidth;
  }, [transcript, state]);

  // The native global hook owns this signal. A WebView key listener only works
  // while the dock itself has focus and creates misleading feedback when the
  // user is typing in another application.
  useEffect(() => {
    const element = dockRef.current;
    if (!element) return;
    if (shortcutArmed && state === "rest") element.setAttribute("data-armed", "");
    else element.removeAttribute("data-armed");
  }, [shortcutArmed, state]);

  // press → potential OS-window drag; a clean tap on the body starts dictation
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button != null && e.button !== 0) return;
    drag.current = { sx: e.clientX, sy: e.clientY, moved: false };
    // Without capture, a quick drag leaves the 66px orb before the 4px threshold
    // registers and the move is lost entirely.
    try {
      dockRef.current?.setPointerCapture(e.pointerId);
    } catch {
      /* capture is best-effort */
    }
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const ds = drag.current;
    if (!ds || ds.moved) return;
    if (Math.hypot(e.clientX - ds.sx, e.clientY - ds.sy) > 4) {
      ds.moved = true;
      dockRef.current?.setAttribute("data-dragging", "");
      // Capture is kept: the host drives the window itself from subsequent
      // pointer events, so it needs them to keep arriving here.
      onMoveStart?.(e);
    }
  };
  const onPointerUp = (e: React.PointerEvent) => {
    const ds = drag.current;
    drag.current = null;
    dockRef.current?.removeAttribute("data-dragging");
    try {
      dockRef.current?.releasePointerCapture(e.pointerId);
    } catch {
      /* already released */
    }
    if (!ds) return;
    if (ds.moved) return; // it was a drag, not a tap
    if (state === "rest") onToggle(); // clean tap on the body → start dictation
  };

  const timer = `${String(Math.floor(elapsedSeconds / 60)).padStart(2, "0")}:${String(
    Math.floor(elapsedSeconds % 60),
  ).padStart(2, "0")}`;
  // The handoff hardcodes "hold ⌥space"; this is Windows-only and the chord is
  // user-configurable, so the tip has to follow the real settings.
  const tip = mode === "toggle" ? "tap to speak" : `hold ${hotkeyLabel}`;
  // Per the handoff, the orb sits smaller at rest than the capsule's inline mark.
  const auraSize = state === "rest" ? (dockShape === "orb" ? 34 : 28) : 40;

  const wrapStyle: CSSProperties = {
    "--accent": pigment.accent,
    "--accent-soft": pigment.soft,
    "--accent-deep": pigment.deep,
    "--accent-glow": pigment.glow,
    "--neon": pigment.neon,
    "--dock-scale": BUBBLE_SCALE[bubbleSize] ?? BUBBLE_SCALE.medium,
    "--dur-breath": motion === "calm" ? "8s" : "5s",
    opacity: bubbleOpacity != null && bubbleOpacity > 0 ? bubbleOpacity : undefined,
  } as CSSProperties;

  const modelReady = modelStatus?.ready ?? true;

  return (
    <div className={clsx("dock-wrap", theme === "dark" && "dark-bg")} style={wrapStyle}>
      <div
        className="dock"
        ref={dockRef}
        data-state={state}
        data-shape={dockShape}
        data-size={bubbleSize}
        data-theme={theme}
        data-tauri-drag-region
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onDoubleClick={onOpenHub}
        title={state === "rest" ? "Tap to dictate · drag to move · double-click for hub" : "Drag to move"}
        role="button"
        tabIndex={0}
        aria-label="Atmospeak companion — tap to dictate, drag to move"
      >
        <span className="dock__mark" data-tauri-drag-region>
          <Aura size={auraSize} active={listening} />
        </span>

        {state === "rest" && dockShape !== "orb" && (
          <span className="dock__restlabel" data-tauri-drag-region>{modelReady ? tip : "runtime offline"}</span>
        )}

        {state !== "rest" && (
          <div className="dock__core" data-tauri-drag-region>
            {state === "processing" ? (
              <div className="dock__transcript proc">
                <span className="shim">transcribing on device</span>
              </div>
            ) : state === "error" ? (
              <div className="dock__transcript">
                <span className="partial">{notice || "Could not finish this dictation"}</span>
              </div>
            ) : state === "delivered" ? (
              <div className="dock__transcript">
                <span className="stable">
                  {resolvedPhase === "saved" ? "Saved to history" : `Set down in ${hostApp}`}
                </span>
              </div>
            ) : (
              <div className="dock__transcript" ref={txRef}>
                {transcript ? (
                  <>
                    {stableTranscript && <span className="stable">{stableTranscript}</span>}
                    {stableTranscript && partialTranscript && " "}
                    {partialTranscript && <span className="partial">{partialTranscript}</span>}
                  </>
                ) : (
                  <span className="placeholder">listening — speak naturally…</span>
                )}
              </div>
            )}
            {listening && (
              <WaveCanvas
                levelRef={levelRef}
                style={waveStyle}
                active
                accent={pigment.accent}
                accentSoft={pigment.neon}
              />
            )}
          </div>
        )}

        {listening && (
          <div className="dock__right">
            <span className="dock__timer">{timer}</span>
            <button
              className="dock__discard"
              type="button"
              onPointerDown={(e) => e.stopPropagation()}
              onClick={onCancel}
              title="Discard"
              aria-label="Discard this dictation"
            >
              <UndoGlyph size={16} />
            </button>
            <button
              className="dock__insert"
              type="button"
              onPointerDown={(e) => e.stopPropagation()}
              onClick={onToggle}
              disabled={busy}
              title="Insert at cursor"
              aria-label="Insert text at the cursor"
            >
              <InsertGlyph size={16} />
              <span className="lbl">Insert</span>
            </button>
          </div>
        )}
        {state === "delivered" && (
          <div className="dock__right">
            <span className="dock__delivered">
              <CheckGlyph size={15} />
            </span>
          </div>
        )}
      </div>

      {/* The orb carries its tip underneath; the wider shapes carry it inline. */}
      {state === "rest" && dockShape === "orb" && (
        <div className={clsx("dock-tip", !modelReady && "dock-tip--warn")}>{modelReady ? tip : "runtime offline"}</div>
      )}
      {notice && <div className="dock-alert" role="status">{notice}</div>}
    </div>
  );
}

export const RecorderOverlay = memo(RecorderOverlayComponent);
