import { copyText } from "../lib/api";
import { useAppSnapshot } from "../lib/useAppSnapshot";
import { defaultSettings, type AppSnapshot } from "../types/dictation";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
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

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (value: string) => "http://asset.localhost/" + encodeURIComponent(value),
}));

describe("HistoryPanel", () => {
  it("keeps audio-only captures accessible without copying empty text", () => {
    render(<HistoryPanel {...historyProps([session({
      rawText: "", cleanedText: "", audioPath: "C:/recordings/recovered.wav",
      injected: false, wordCount: 0,
    })])} />);
    fireEvent.click(screen.getByRole("button", { name: "Audio saved. Open to listen." }));
    const audio = document.querySelector("audio");
    expect(audio).toHaveAttribute("src", "http://asset.localhost/C%3A%2Frecordings%2Frecovered.wav");
    expect(screen.getByRole("button", { name: /^Copy$/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Paste again/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /AI polish/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Delete/ })).toBeEnabled();
  });
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
        "No matching transcripts. Try another search.",
      ),
    ).toBeVisible();
  });

  it("reveals row actions only after the transcript is expanded", () => {
    const props = historyProps([session()]);
    render(<HistoryPanel {...props} />);

    expect(screen.queryByRole("button", { name: /^Copy$/ })).not.toBeInTheDocument();

    const row = screen.getByRole("button", { name: "Hello world." });
    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(screen.getByRole("button", { name: /^Copy$/ }));
    fireEvent.click(screen.getByRole("button", { name: /Paste again/ }));
    fireEvent.click(screen.getByRole("button", { name: /AI polish/ }));
    fireEvent.click(screen.getByRole("button", { name: /Delete/ }));

    expect(props.onCopy).toHaveBeenCalledWith(expect.objectContaining({ id: "session-1" }));
    expect(props.onInject).toHaveBeenCalledOnce();
    expect(props.onPolish).toHaveBeenCalledOnce();
    expect(props.onDelete).toHaveBeenCalledOnce();

    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /^Copy$/ })).not.toBeInTheDocument();
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

  it("calls onInject with the polished session when Paste again is used", () => {
    const props = historyProps([
      session({ polishedText: "Hello, world!", preferPolished: true }),
    ]);
    render(<HistoryPanel {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Hello, world!" }));
    fireEvent.click(screen.getByRole("button", { name: /Paste again/ }));
    expect(props.onInject).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "session-1",
        preferPolished: true,
        polishedText: "Hello, world!",
      }),
    );
  });
});


describe("History snapshot ownership", () => {
  function snapshot(sessions: TranscriptSession[]): AppSnapshot {
    return { settings: defaultSettings(), sessions, dictionary: [], snippets: [],
      stats: { totalSessions: sessions.length, totalWords: 0, totalDurationMs: 0, averageWordsPerMinute: 0 } };
  }

  function pendingSnapshot() {
    let resolve!: (value: AppSnapshot) => void;
    const promise = new Promise<AppSnapshot>((done) => { resolve = done; });
    return { promise, resolve };
  }

  it("keeps the newest refresh when responses arrive out of order", async () => {
    const first = pendingSnapshot();
    const second = pendingSnapshot();
    const load = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useAppSnapshot(load));
    let older!: Promise<AppSnapshot>;
    let newer!: Promise<AppSnapshot>;
    act(() => { older = result.current.refreshSnapshot(); newer = result.current.refreshSnapshot(); });
    const latest = snapshot([session({ polishedText: "New result" })]);
    await act(async () => { second.resolve(latest); await newer; });
    await act(async () => { first.resolve(snapshot([])); await older; });
    expect(result.current.snapshot).toEqual(latest);
  });

  it("does not resurrect a deleted transcript from a pending refresh", async () => {
    const pending = pendingSnapshot();
    const load = vi.fn(() => pending.promise);
    const { result } = renderHook(() => useAppSnapshot(load));
    let refresh!: Promise<AppSnapshot>;
    act(() => { refresh = result.current.refreshSnapshot(); result.current.setSnapshot(snapshot([])); });
    await act(async () => { pending.resolve(snapshot([session()])); await refresh; });
    expect(result.current.snapshot?.sessions).toEqual([]);
  });

  it("retains existing History after refresh failure and accepts a later retry", async () => {
    const saved = snapshot([session()]);
    const load = vi.fn<() => Promise<AppSnapshot>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(saved);
    const { result } = renderHook(() => useAppSnapshot(load));
    act(() => result.current.setSnapshot(saved));
    await act(async () => { await expect(result.current.refreshSnapshot()).rejects.toThrow("offline"); });
    expect(result.current.snapshot).toEqual(saved);
    await act(async () => { await result.current.refreshSnapshot(); });
    expect(result.current.snapshot).toEqual(saved);
  });
});


vi.mock("../lib/api", () => ({ getAppSnapshot: vi.fn(), hasTauriRuntime: () => false, copyText: vi.fn().mockResolvedValue("Copied.") }));

describe("Transcript comparison and recovery selection", () => {
  it("compares and copies versions without changing the saved preference", async () => {
    const props = historyProps([session({ rawText: "um hello", cleanedText: "Hello.", polishedText: "Hello there.", preferPolished: true })]);
    render(<HistoryPanel {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Hello there." }));
    fireEvent.click(screen.getByText("Compare transcript versions"));
    expect(screen.getByLabelText("Original transcript")).toHaveTextContent("um hello");
    fireEvent.click(screen.getByRole("button", { name: "Cleaned" }));
    expect(screen.getByLabelText("Cleaned transcript")).toHaveTextContent("Hello.");
    fireEvent.click(screen.getByRole("button", { name: "AI edited" }));
    expect(screen.getByLabelText("AI edited transcript")).toHaveTextContent("Hello there.");
    fireEvent.click(screen.getByRole("button", { name: "Original" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copy original" })));
    expect(copyText).toHaveBeenCalledWith("um hello");
    expect(props.onUndoAiEdit).not.toHaveBeenCalled();
    expect(props.onRedoAiEdit).not.toHaveBeenCalled();
  });

  it("opens the requested recording instead of an unrelated newer transcript", () => {
    render(<HistoryPanel {...historyProps([session({ id: "new", cleanedText: "Newer text" }), session({ id: "failed", rawText: "", cleanedText: "" })])} selection={{ sessionId: "failed" }} />);
    const requested = screen.getByRole("button", { name: "Audio saved. Open to listen." });
    expect(requested).toHaveAttribute("aria-expanded", "true");
    expect(requested).toHaveFocus();
    expect(screen.getByRole("button", { name: "Newer text" })).toHaveAttribute("aria-expanded", "false");
  });
});

it("focuses recovery once, preserves search focus on refresh, and focuses a new request", async () => {
  const item = session({ id: "saved", cleanedText: "Café" });
  const selection = { sessionId: "saved" };
  const props = historyProps([item]);
  const { rerender } = render(<HistoryPanel {...props} selection={selection} />);
  expect(screen.getByRole("button", { name: "Café" })).toHaveFocus();
  const search = screen.getByPlaceholderText(/Search transcripts/);
  search.focus();
  fireEvent.change(search, { target: { value: "CAFÉ" } });
  rerender(<HistoryPanel {...props} sessions={[{ ...item }]} selection={selection} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Café" })).toBeVisible());
  expect(search).toHaveFocus();
  rerender(<HistoryPanel {...props} selection={{ sessionId: "saved" }} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Café" })).toHaveFocus());
});
