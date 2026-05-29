import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { RecorderOverlay } from "./RecorderOverlay";

describe("RecorderOverlay", () => {
  it("fires the primary dictation action", async () => {
    const onToggle = vi.fn();
    render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy={false}
        modelStatus={{
          whisperCliFound: true,
          modelFound: true,
          ready: true,
          message: "Ready",
          source: "bundled",
          whisperCliPath: "app://runtime/whisper-cli.exe",
          modelPath: "app://models/ggml-base.en.bin",
        }}
        onToggle={onToggle}
        onCancel={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Dictate" }));

    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
