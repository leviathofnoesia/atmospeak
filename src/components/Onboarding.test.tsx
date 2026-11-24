import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { defaultSettings } from "../types/dictation";
import type { SoundCheckResult } from "../types/dictation";
import { Onboarding } from "./Onboarding";

const passingSoundCheck: SoundCheckResult = {
  passed: true,
  failureCode: null,
  deviceName: "Elgato Wave:3",
  captureFormat: "mono f32 48000Hz -> mono PCM16 16000Hz",
  durationMs: 4200,
  activeSpeechMs: 2800,
  rmsDbfs: -24,
  peakDbfs: -8,
  noiseFloorDbfs: -52,
  snrDb: 28,
  clippingRatio: 0,
  transcript: "The porcelain moon hums over the studio.",
  expectedPhrase: "The porcelain moon hums over the studio.",
  tokenSimilarity: 1,
  asrBackend: "host",
  modelId: "parakeet-unified-en",
  captureMs: 20,
  asrMs: 340,
  totalMs: 360,
};

function onboardingProps(overrides: Record<string, unknown> = {}) {
  return {
    settings: { ...defaultSettings(), microphoneName: "Elgato Wave:3" },
    setSettings: vi.fn(),
    microphones: [
      {
        name: "Elgato Wave:3",
        isDefault: true,
        isSelected: true,
        available: true,
      },
    ],
    modelStatus: {
      executableFound: true,
      modelFound: true,
      ready: true,
      message: "Ready",
      source: "bundled" as const,
      executablePath: "atmospeak-asr-cpu.exe",
      modelPath: "models/parakeet-unified-en-0.6b-Q8_0.gguf",
    },
    modelInventory: {
      activeModelId: "parakeet-unified-en",
      models: [
        {
          id: "parakeet-unified-en",
          label: "Parakeet Unified English 0.6B",
          installed: true,
          bundled: true,
          path: "bundled",
          sizeMb: 269,
        },
      ],
    },
    modelDownload: null,
    shortcutStatus: null,
    shortcutTest: { active: false, detected: false, message: "" },
    shortcutCapture: { arming: false, active: false, keys: [], message: "" },
    micCheck: { active: false, passed: true, level: 0.7, message: "Healthy signal" },
    soundCheck: { active: false, result: passingSoundCheck, message: "Heard clearly" },
    onStartMicCheck: vi.fn(async () => undefined),
    onStopMicCheck: vi.fn(async () => undefined),
    onStartSoundCheck: vi.fn(async () => undefined),
    onFinishSoundCheck: vi.fn(async () => undefined),
    onOpenWindowsSoundSettings: vi.fn(async () => undefined),
    onTestShortcut: vi.fn(),
    onRecordShortcut: vi.fn(),
    onCancelShortcutTest: vi.fn(),
    onShortcutChange: vi.fn(),
    pasteTest: { running: false, passed: false, message: "" },
    onPasteTest: vi.fn(async () => undefined),
    onSelectModel: vi.fn(),
    onDownloadModel: vi.fn(async () => undefined),
    onCancelModelDownload: vi.fn(async () => undefined),
    onComplete: vi.fn(async () => undefined),
    ...overrides,
  };
}

describe("Onboarding", () => {
  it("has no skip path and uses the bundled speech engine", async () => {
    const user = userEvent.setup();
    render(<Onboarding {...onboardingProps()} />);

    expect(screen.queryByText(/skip setup/i)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    expect(screen.getByText("Parakeet Unified English 0.6B")).toBeVisible();
    expect(screen.queryByText("Swift")).not.toBeInTheDocument();
  });

  it("will not pass the shortcut step until the native chord is detected", async () => {
    const user = userEvent.setup();
    const props = onboardingProps();
    render(<Onboarding {...props} />);

    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    const continueButton = screen.getByRole("button", { name: /continue/i });
    expect(continueButton).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /test selected/i }));
    expect(props.onTestShortcut).toHaveBeenCalledOnce();
    expect(props.onComplete).not.toHaveBeenCalled();
  });

  it("shows an armed shortcut test and cancels it when leaving the step", async () => {
    const user = userEvent.setup();
    const props = onboardingProps({
      shortcutTest: {
        active: true,
        detected: false,
        message: "Press your dictation shortcut...",
      },
    });
    render(<Onboarding {...props} />);

    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    expect(screen.getByRole("button", { name: /testing/i })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /back/i }));
    expect(props.onCancelShortcutTest).toHaveBeenCalledOnce();
  });

  it("invalidates the previous shortcut test when the selected chord changes", async () => {
    const user = userEvent.setup();
    const props = onboardingProps();
    render(<Onboarding {...props} />);

    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: "Ctrl+Alt+D" }));

    expect(props.onShortcutChange).toHaveBeenCalledWith("Ctrl+Alt+D");
  });

  it("records custom chords and lights each held key like a keyboard tester", async () => {
    const user = userEvent.setup();
    const props = onboardingProps({
      shortcutCapture: {
        arming: false,
        active: true,
        keys: ["Ctrl", "Shift", "K"],
        message: "Hold the keys you want together.",
      },
    });
    render(<Onboarding {...props} />);

    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    for (const key of ["Ctrl", "Shift", "K"]) {
      expect(screen.getByText(key, { selector: "kbd" })).toHaveClass("is-down");
    }
    expect(screen.getByRole("button", { name: /recording keys/i })).toBeDisabled();
  });

  it("offers a real retry after a failed host-backed sound check", async () => {
    const user = userEvent.setup();
    const failedResult = {
      ...passingSoundCheck,
      passed: false,
      failureCode: "too_quiet",
      transcript: "",
    };
    render(
      <Onboarding
        {...onboardingProps({
          shortcutTest: { active: false, detected: true, message: "Detected" },
          soundCheck: {
            active: false,
            result: failedResult,
            message: "The microphone is too quiet. Move closer and retry.",
          },
        })}
      />,
    );

    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    expect(screen.getByRole("button", { name: /hold to read/i })).toBeVisible();
    expect(screen.getByText(/too quiet/i)).toBeVisible();
    expect(screen.getByRole("button", { name: /continue/i })).toBeDisabled();
  });

  it("starts on press and finishes on release on the porcelain phrase control", async () => {
    const user = userEvent.setup();
    const props = onboardingProps({
      shortcutTest: { active: false, detected: true, message: "Detected" },
      soundCheck: {
        active: false,
        result: { ...passingSoundCheck, passed: false, failureCode: "too_quiet" },
        message: "Retry.",
      },
    });
    render(<Onboarding {...props} />);

    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    const hold = screen.getByRole("button", { name: /hold to read/i });
    hold.setPointerCapture = vi.fn();
    fireEvent.pointerDown(hold, { pointerId: 7 });
    expect(hold.setPointerCapture).toHaveBeenCalledWith(7);
    expect(props.onStartSoundCheck).toHaveBeenCalledOnce();

    fireEvent.pointerUp(hold, { pointerId: 7 });
    expect(props.onFinishSoundCheck).toHaveBeenCalledOnce();
  });
});


it("lets a failed sound check change microphones in place and rejects an old device pass", async () => {
  const user = userEvent.setup();
  const props = onboardingProps({ shortcutTest: { active: false, detected: true, message: "Detected" }, microphones: [
    { name: "Elgato Wave:3", isDefault: true, isSelected: true, available: true },
    { name: "Headset", isDefault: false, isSelected: false, available: true },
  ], soundCheck: { active: false, result: { ...passingSoundCheck, passed: false, failureCode: "too_quiet" }, message: "No speech heard" } });
  const { rerender } = render(<Onboarding {...props} />);
  await user.click(screen.getByRole("button", { name: /begin/i }));
  for (let step = 0; step < 3; step++) await user.click(screen.getByRole("button", { name: /continue/i }));
  await user.selectOptions(screen.getByRole("combobox", { name: "Sound-check microphone" }), "Headset");
  expect(props.setSettings).toHaveBeenCalledWith(expect.objectContaining({ microphoneName: "Headset" }));
  await user.click(screen.getByRole("button", { name: "Open microphone settings" }));
  expect(props.onOpenWindowsSoundSettings).toHaveBeenCalledOnce();
  rerender(<Onboarding {...props} settings={{ ...props.settings, microphoneName: "Headset" }} soundCheck={{ active: false, result: passingSoundCheck, message: "Earlier device passed" }} />);
  expect(screen.getByRole("button", { name: /continue/i })).toBeDisabled();
});
