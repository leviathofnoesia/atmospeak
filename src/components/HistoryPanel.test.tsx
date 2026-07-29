import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TranscriptSession } from "../types/dictation";
import { HistoryPanel } from "./HistoryPanel";

function session(overrides: Partial<TranscriptSession> = {}): TranscriptSession {
  return {
    id: "session-1",
    rawText: "hello world",
    cleanedText: "Hello world.",
    polishedText: null,
    preferPolished: false,
    audioPath: "mock://audio",
    durationMs: 4200,
    wordCount: 2,
    injected: true,
    sourceApplication: "Notepad",
    createdAt: "2026-06-01T12:00:00.000Z",
    ...overrides,
  };
}

function historyProps(sessions: TranscriptSession[]) {
  return {
    sessions,
    onCopy: vi.fn(async () => undefined),
    onInject: vi.fn(async () => undefined),
    onDelete: vi.fn(async () => undefined),
    onPolish: vi.fn(async () => undefined),
    onUndoAiEdit: vi.fn(async () => undefined),
    onRedoAiEdit: vi.fn(async () => undefined),
  };
}

describe("HistoryPanel", () => {
  it("shows the empty state when there are no sessions", () => {
    render(<HistoryPanel {...historyProps([])} />);

    expect(
      screen.getByText(
        "No transcripts yet. Dictations appear here after a successful session.",
      ),
    ).toBeVisible();
  });

  it("filters sessions by cleaned and polished text", () => {
    const sessions = [
      session({ id: "a", cleanedText: "Ship the release notes." }),
      session({
        id: "b",
        cleanedText: "raw wording",
        polishedText: "Polished quarterly summary.",
      }),
    ];
    render(<HistoryPanel {...historyProps(sessions)} />);

    fireEvent.change(screen.getByPlaceholderText("Search transcripts…"), {
      target: { value: "quarterly" },
    });

    expect(screen.getByRole("button", { name: "raw wording" })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Ship the release notes." }),
    ).not.toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search transcripts…"), {
      target: { value: "nothing matches this" },
    });
    expect(
      screen.getByText(
        "No transcripts yet. Dictations appear here after a successful session.",
      ),
    ).toBeVisible();
  });

  it("reveals row actions only after the transcript is expanded", () => {
    const props = historyProps([session()]);
    render(<HistoryPanel {...props} />);

    expect(screen.queryByRole("button", { name: /Copy/ })).not.toBeInTheDocument();

    const row = screen.getByRole("button", { name: "Hello world." });
    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(screen.getByRole("button", { name: /Copy/ }));
    fireEvent.click(screen.getByRole("button", { name: /Paste again/ }));
    fireEvent.click(screen.getByRole("button", { name: /AI polish/ }));
    fireEvent.click(screen.getByRole("button", { name: /Delete/ }));

    expect(props.onCopy).toHaveBeenCalledWith(expect.objectContaining({ id: "session-1" }));
    expect(props.onInject).toHaveBeenCalledOnce();
    expect(props.onPolish).toHaveBeenCalledOnce();
    expect(props.onDelete).toHaveBeenCalledOnce();

    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /Copy/ })).not.toBeInTheDocument();
  });

  it("offers undo for a polished session that is currently showing the AI edit", () => {
    const props = historyProps([
      session({ polishedText: "Hello, world!", preferPolished: true }),
    ]);
    render(<HistoryPanel {...props} />);

    fireEvent.click(screen.getByRole("button", { name: "Hello, world!" }));
    expect(screen.queryByRole("button", { name: /Redo AI edit/ })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Undo AI edit/ }));
    expect(props.onUndoAiEdit).toHaveBeenCalledOnce();
  });

  it("offers redo for a polished session that is showing the original text", () => {
    const props = historyProps([
      session({ polishedText: "Hello, world!", preferPolished: false }),
    ]);
    render(<HistoryPanel {...props} />);

    fireEvent.click(screen.getByRole("button", { name: "Hello world." }));
    expect(screen.queryByRole("button", { name: /Undo AI edit/ })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Redo AI edit/ }));
    expect(props.onRedoAiEdit).toHaveBeenCalledOnce();
  });

  it("numbers rows newest-first and reports duration in seconds", () => {
    render(
      <HistoryPanel
        {...historyProps([
          session({ id: "a", cleanedText: "Newest", durationMs: 4200 }),
          session({ id: "b", cleanedText: "Oldest", durationMs: 1000 }),
        ])}
      />,
    );

    expect(screen.getByText("02")).toBeVisible();
    expect(screen.getByText("01")).toBeVisible();
    expect(screen.getByText(/2 w · 4s/)).toBeVisible();
    expect(screen.getByText(/2 w · 1s/)).toBeVisible();
  });
});
