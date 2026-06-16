import { render, screen } from "@testing-library/react";
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
    expect(screen.getByRole("button", { name: /Atmospeak companion/i })).toHaveAttribute(
      "data-shape",
      "capsule",
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
});
