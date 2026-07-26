import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { RecorderOverlay } from "./RecorderOverlay";

describe("RecorderOverlay", () => {
  const readyModel = {
    whisperCliFound: true,
    modelFound: true,
    ready: true,
    message: "Ready",
    source: "bundled" as const,
    whisperCliPath: "app://runtime/whisper-cli.exe",
    modelPath: "app://models/ggml-base.en.bin",
  };

  const recording = {
    id: "rec-1",
    startedAt: new Date().toISOString(),
    microphoneName: "Studio Mic",
  };

  it("starts dictation on a clean tap of the resting orb", async () => {
    const onToggle = vi.fn();
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="idle"
        modelStatus={readyModel}
        onToggle={onToggle}
        onCancel={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /Atmospeak companion/i }));

    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("maps saved bubble appearance settings onto the redesigned dock", () => {
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="idle"
        modelStatus={readyModel}
        bubbleSize="large"
        bubbleOpacity={0.55}
        onToggle={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    const dock = screen.getByRole("button", { name: /Atmospeak companion/i });
    expect(dock).toHaveAttribute("data-shape", "orb");
    expect(dock).toHaveAttribute("data-size", "large");
    expect(dock.parentElement).toHaveStyle({ opacity: "0.55" });
  });

  it("exposes Insert and Discard while listening", async () => {
    const onToggle = vi.fn();
    const onCancel = vi.fn();
    render(
      <RecorderOverlay
        recording={recording}
        elapsedSeconds={5}
        busy={false}
        phase="listening"
        modelStatus={readyModel}
        liveTranscript={{ sessionId: "rec-1", phase: "stable", text: "hello there", latencyMs: null }}
        onToggle={onToggle}
        onCancel={onCancel}
      />,
    );

    expect(screen.getByText("hello there")).toBeVisible();
    // The active capsule geometry is driven by data-state; data-shape stays the
    // user's chosen *resting* silhouette and must not be rewritten while listening.
    expect(screen.getByRole("button", { name: /Atmospeak companion/i })).toHaveAttribute(
      "data-state",
      "listening",
    );
    expect(screen.getByRole("button", { name: /Atmospeak companion/i })).toHaveAttribute(
      "data-shape",
      "orb",
    );

    await userEvent.click(screen.getByRole("button", { name: /Insert text at the cursor/i }));
    expect(onToggle).toHaveBeenCalledTimes(1);

    await userEvent.click(screen.getByRole("button", { name: /Discard this dictation/i }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("shows processing and delivered phases", () => {
    const { rerender } = render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy
        phase="processing"
        modelStatus={readyModel}
        onToggle={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByText("transcribing on device")).toBeVisible();

    rerender(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="pasted"
        modelStatus={readyModel}
        hostApp="Letters"
        onToggle={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByText(/Set down in Letters/i)).toBeVisible();
  });

  it("carries the resting tip underneath the orb and inline on wider shapes", () => {
    const base = {
      recording: null,
      elapsedSeconds: 0,
      busy: false,
      phase: "idle" as const,
      modelStatus: readyModel,
      hotkeyLabel: "Ctrl+Win",
      onToggle: vi.fn(),
      onCancel: vi.fn(),
    };

    const orb = render(<RecorderOverlay {...base} dockShape="orb" />);
    expect(document.querySelector(".dock")?.getAttribute("data-shape")).toBe("orb");
    expect(document.querySelector(".dock-tip")?.textContent).toBe("hold Ctrl+Win");
    expect(document.querySelector(".dock__restlabel")).toBeNull();
    orb.unmount();

    // Capsule and tape rest wider, so the tip moves inside the dock.
    const tape = render(<RecorderOverlay {...base} dockShape="tape" />);
    expect(document.querySelector(".dock")?.getAttribute("data-shape")).toBe("tape");
    expect(document.querySelector(".dock__restlabel")?.textContent).toBe("hold Ctrl+Win");
    expect(document.querySelector(".dock-tip")).toBeNull();
    tape.unmount();
  });

  it("names the real chord and gesture rather than the handoff's macOS default", () => {
    const base = {
      recording: null,
      elapsedSeconds: 0,
      busy: false,
      phase: "idle" as const,
      modelStatus: readyModel,
      onToggle: vi.fn(),
      onCancel: vi.fn(),
    };

    const hold = render(<RecorderOverlay {...base} hotkeyLabel="Ctrl+Alt" mode="pushToTalk" />);
    expect(screen.getByText("hold Ctrl+Alt")).toBeTruthy();
    hold.unmount();

    // Toggle mode has nothing to hold, so the tip changes verb.
    const toggle = render(<RecorderOverlay {...base} hotkeyLabel="Ctrl+Win" mode="toggle" />);
    expect(screen.getByText("tap to speak")).toBeTruthy();
    toggle.unmount();
  });

  it("greys the tip out when the speech runtime is missing", () => {
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="idle"
        modelStatus={{ ...readyModel, ready: false, message: "Model missing" }}
        hotkeyLabel="Ctrl+Win"
        onToggle={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    // It must not invite a dictation that cannot run.
    expect(screen.getByText("runtime offline")).toBeTruthy();
  });

  it("does not create an animation frame loop while idle", () => {
    const requestFrame = vi.spyOn(window, "requestAnimationFrame");
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="idle"
        modelStatus={readyModel}
        onToggle={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(requestFrame).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /Atmospeak companion/i })).toHaveAttribute(
      "data-tauri-drag-region",
    );
    requestFrame.mockRestore();
  });

  it("starts the native drag path after the four-pixel threshold", () => {
    const onMoveStart = vi.fn();
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="idle"
        modelStatus={readyModel}
        onToggle={vi.fn()}
        onCancel={vi.fn()}
        onMoveStart={onMoveStart}
      />,
    );

    const dock = screen.getByRole("button", { name: /Atmospeak companion/i });
    fireEvent.pointerDown(dock, { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(dock, { pointerId: 1, clientX: 16, clientY: 10 });
    expect(onMoveStart).toHaveBeenCalledTimes(1);
  });
});
