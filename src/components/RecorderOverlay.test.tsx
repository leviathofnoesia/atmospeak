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

  it("fires the primary dictation action", async () => {
    const onToggle = vi.fn();
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        modelStatus={readyModel}
        onToggle={onToggle}
        onCancel={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Dictate" }));

    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("shows processing and pasted recorder phases", () => {
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

    expect(screen.getByRole("heading", { name: "Processing" })).toBeVisible();
    expect(screen.getByText("Transcribing locally")).toBeVisible();

    rerender(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        phase="pasted"
        modelStatus={readyModel}
        onToggle={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Pasted" })).toBeVisible();
    expect(screen.getByText("Transcript delivered")).toBeVisible();
  });
});
