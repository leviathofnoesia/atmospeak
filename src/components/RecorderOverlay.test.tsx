import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { RecorderOverlay } from "./RecorderOverlay";

describe("RecorderOverlay", () => {
  const readyModel = {
    executableFound: true,
    modelFound: true,
    ready: true,
    message: "Ready",
    source: "bundled" as const,
    executablePath: "app://runtime/atmospeak-asr-cpu.exe",
    modelPath: "app://resources/models/parakeet-unified-en-0.6b-Q8_0.gguf",
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

  it("renders the medium dock with full opacity", () => {
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

    const dock = screen.getByRole("button", { name: /Atmospeak companion/i });
    expect(dock).toHaveAttribute("data-shape", "orb");
    expect(dock).toHaveAttribute("data-size", "medium");
    expect(dock.parentElement).toHaveStyle({ opacity: "1" });
  });

  it("exposes Stop and Cancel while listening", async () => {
    const onToggle = vi.fn();
    const onCancel = vi.fn();
    render(
      <RecorderOverlay
        recording={recording}
        elapsedSeconds={5}
        busy={false}
        phase="listening"
        modelStatus={readyModel}
        liveTranscript={{
          sessionId: "rec-1",
          phase: "partial",
          stableText: "hello",
          partialText: "there",
          latencyMs: 800,
        }}
        onToggle={onToggle}
        onCancel={onCancel}
      />,
    );

    expect(screen.getByText("hello")).toBeVisible();
    expect(screen.getByText("there")).toBeVisible();
    // The active capsule geometry is driven by data-state; data-shape stays the
    // user's chosen *resting* silhouette and must not be rewritten while listening.
    expect(screen.getByRole("group", { name: /Atmospeak companion/i })).toHaveAttribute(
      "data-state",
      "listening",
    );
    expect(screen.getByRole("group", { name: /Atmospeak companion/i })).toHaveAttribute(
      "data-shape",
      "orb",
    );

    await userEvent.click(screen.getByRole("button", { name: /Stop and insert/i }));
    expect(onToggle).toHaveBeenCalledTimes(1);

    await userEvent.click(screen.getByRole("button", { name: /Cancel recording/i }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("shows processing and delivered phases", () => {
    const { rerender } = render(
      <RecorderOverlay
        recording={null}
        elapsedSeconds={0}
        busy
        phase="finalizing"
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


it("offers explicit recovery and dismissal after an error without starting capture", () => {
  const copy = vi.fn(), open = vi.fn(), dismiss = vi.fn(), toggle = vi.fn();
  render(<RecorderOverlay recording={null} elapsedSeconds={0} busy={false} phase="error" modelStatus={null}
    onToggle={toggle} onCancel={vi.fn()} hasRecovery onCopyTranscript={copy} onOpenHistory={open} onDismissError={dismiss} />);
  fireEvent.click(screen.getByRole("button", { name: "Copy transcript" }));
  fireEvent.click(screen.getByRole("button", { name: "Open saved recording" }));
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(copy).toHaveBeenCalledOnce(); expect(open).toHaveBeenCalledOnce(); expect(dismiss).toHaveBeenCalledOnce();
  expect(toggle).not.toHaveBeenCalled();
});

it("supports keyboard start at rest and does not offer an unrelated copy after a setup failure", () => {
  const toggle = vi.fn();
  const { rerender } = render(<RecorderOverlay recording={null} elapsedSeconds={0} busy={false} modelStatus={null} onToggle={toggle} onCancel={vi.fn()} />);
  fireEvent.keyDown(screen.getByRole("button", { name: /Atmospeak companion/ }), { key: "Enter" });
  expect(toggle).toHaveBeenCalledOnce();
  rerender(<RecorderOverlay recording={null} elapsedSeconds={0} busy={false} phase="error" modelStatus={null} onToggle={toggle} onCancel={vi.fn()} onCopyTranscript={vi.fn()} onOpenHistory={vi.fn()} />);
  expect(screen.queryByRole("button", { name: /Copy/ })).toBeNull();
  expect(screen.getByRole("button", { name: "Open History" })).toBeVisible();
});


vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../lib/api", () => ({ getDictationSnapshot: vi.fn() }));

describe("recording state hydration", () => {
  it("hydrates a failed recording for a new window", async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const { getDictationSnapshot } = await import("../lib/api");
    const { watchDictation } = await import("../lib/dictationState");
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValue(unlisten);
    const failure = { generation: 2, phase: "error" as const, recording: null, message: "Saved failure", result: null, metrics: null };
    vi.mocked(getDictationSnapshot).mockResolvedValue(failure);
    const accept = vi.fn();
    const stop = await watchDictation(accept);
    expect(accept).toHaveBeenCalledWith(failure);
    stop();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("ignores old hydration after a newer event and ignores duplicate delivery", async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const { getDictationSnapshot } = await import("../lib/api");
    const { watchDictation } = await import("../lib/dictationState");
    const newer = { generation: 4, phase: "listening" as const, recording: null, message: "Listening", result: null, metrics: null };
    vi.mocked(listen).mockResolvedValue(vi.fn());
    vi.mocked(getDictationSnapshot).mockResolvedValue({ ...newer, generation: 3, phase: "error" });
    const accept = vi.fn();
    const pending = watchDictation(accept);
    const receive = vi.mocked(listen).mock.calls[vi.mocked(listen).mock.calls.length - 1][1];
    receive({ event: "wind-speak://native-dictation", id: 1, payload: newer });
    receive({ event: "wind-speak://native-dictation", id: 1, payload: newer });
    const stop = await pending;
    expect(accept).toHaveBeenCalledTimes(1);
    expect(accept).toHaveBeenCalledWith(newer);
    stop();
  });
});

it("keeps live events connected after hydration fails and retries without another listener", async () => {
  const { listen } = await import("@tauri-apps/api/event");
  const { getDictationSnapshot } = await import("../lib/api");
  const { watchDictation } = await import("../lib/dictationState");
  const unlisten = vi.fn();
  vi.mocked(listen).mockResolvedValue(unlisten);
  vi.mocked(getDictationSnapshot).mockRejectedValueOnce(new Error("Offline"));
  const accept = vi.fn();
  const recovery = vi.fn();
  const stop = await watchDictation(accept, recovery);
  await vi.waitFor(() => expect(recovery).toHaveBeenCalledWith({ retry: expect.any(Function) }));
  expect(unlisten).not.toHaveBeenCalled();
  const event = { generation: 6, phase: "listening" as const, recording: null, message: "Listening", result: null, metrics: null };
  const retry = recovery.mock.calls[0][0].retry;
  vi.mocked(getDictationSnapshot).mockResolvedValue(event);
  await retry();
  expect(accept).toHaveBeenCalledWith(event);
  const receive = vi.mocked(listen).mock.calls[vi.mocked(listen).mock.calls.length - 1][1];
  receive({ event: "wind-speak://native-dictation", id: 1, payload: { ...event, generation: 7 } });
  expect(accept).toHaveBeenCalledTimes(2);
  expect(recovery).toHaveBeenLastCalledWith(null);
  stop();
  expect(unlisten).toHaveBeenCalledOnce();
});

it("unsubscribes while hydration is pending and ignores its late failure", async () => {
  const { listen } = await import("@tauri-apps/api/event");
  const { getDictationSnapshot } = await import("../lib/api");
  const { watchDictation } = await import("../lib/dictationState");
  const unlisten = vi.fn();
  vi.mocked(listen).mockResolvedValue(unlisten);
  let reject!: (error: Error) => void;
  vi.mocked(getDictationSnapshot).mockReturnValue(new Promise((_, fail) => { reject = fail; }));
  const recovery = vi.fn();
  const stop = await watchDictation(vi.fn(), recovery);
  stop();
  reject(new Error("Late failure"));
  await Promise.resolve();
  expect(unlisten).toHaveBeenCalledOnce();
  expect(recovery).not.toHaveBeenCalled();
});

it("reveals secondary controls on focus and keeps recording controls minimal", () => {
  const props = {recording: null, elapsedSeconds: 0, busy: false, modelStatus: null, onToggle: vi.fn(), onCancel: vi.fn(), onCopyTranscript: vi.fn(), onOpenHistory: vi.fn()};
  const {rerender} = render(<RecorderOverlay {...props} />);
  expect(screen.queryByRole("button", {name: "Copy last transcript"})).toBeNull();
  fireEvent.focus(screen.getByRole("button", {name: /Atmospeak companion/}));
  expect(screen.getByRole("button", {name: "Copy last transcript"})).toBeVisible();
  fireEvent.click(screen.getByRole("button", {name: "More options"}));
  expect(screen.getByRole("button", {name: "History"})).toBeVisible();
  rerender(<RecorderOverlay {...props} phase="listening" recording={{id:"r",startedAt:new Date().toISOString(),microphoneName:"Mic"}} />);
  expect(screen.queryByRole("button", {name:"More options"})).toBeNull();
  expect(screen.getByRole("button", {name:"Stop and insert"})).toBeVisible();
  expect(screen.getByRole("button", {name:"Cancel recording"})).toBeVisible();
});


it("collapses the menu on window blur even when its button retains DOM focus", async () => {
  render(<RecorderOverlay recording={null} elapsedSeconds={0} busy={false} modelStatus={null}
    onToggle={vi.fn()} onCancel={vi.fn()} onOpenHistory={vi.fn()} />);
  await userEvent.tab();
  await userEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(screen.getByRole("button", { name: "History" })).toBeVisible();
  fireEvent(window, new Event("blur"));
  expect(screen.queryByRole("button", { name: "History" })).toBeNull();
  expect(screen.queryByRole("button", { name: "More options" })).toBeNull();
});

it("shows recovery-action feedback while idle without showing routine status", () => {
  const props = { recording: null, elapsedSeconds: 0, busy: false, modelStatus: null,
    onToggle: vi.fn(), onCancel: vi.fn(), notice: "Ready" };
  const { rerender } = render(<RecorderOverlay {...props} />);
  expect(screen.queryByRole("status")).toBeNull();
  rerender(<RecorderOverlay {...props} actionNotice="Clipboard is unavailable" />);
  expect(screen.getByRole("status")).toHaveTextContent("Clipboard is unavailable");
  rerender(<RecorderOverlay {...props} />);
  expect(screen.queryByRole("status")).toBeNull();
});
