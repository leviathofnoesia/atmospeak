import { render, screen } from "@testing-library/react";
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
  modelId: "base.en",
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
      whisperCliFound: true,
      modelFound: true,
      ready: true,
      message: "Ready",
      source: "bundled" as const,
      whisperCliPath: "whisper-cli.exe",
      modelPath: "ggml-base.en.bin",
    },
    modelInventory: {
      activeModelId: "base.en",
      models: [
        { id: "tiny.en", label: "Swift", installed: false, bundled: false, path: null, sizeMb: 74 },
        { id: "base.en", label: "Balanced", installed: true, bundled: true, path: "base", sizeMb: 142 },
        { id: "small.en", label: "Faithful", installed: false, bundled: false, path: null, sizeMb: 466 },
        { id: "medium.en", label: "Medium", installed: false, bundled: false, path: null, sizeMb: 1500 },
      ],
    },
    modelDownload: null,
    shortcutStatus: null,
    shortcutTest: { active: false, detected: false, message: "" },
    micCheck: { active: false, passed: true, level: 0.7, message: "Healthy signal" },
    soundCheck: { active: false, result: passingSoundCheck, message: "Heard clearly" },
    onStartMicCheck: vi.fn(async () => undefined),
    onStopMicCheck: vi.fn(async () => undefined),
    onStartSoundCheck: vi.fn(async () => undefined),
    onFinishSoundCheck: vi.fn(async () => undefined),
    onOpenWindowsSoundSettings: vi.fn(async () => undefined),
    onTestShortcut: vi.fn(),
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
  it("has no skip path and limits first-run model choices to the promised three", async () => {
    const user = userEvent.setup();
    render(<Onboarding {...onboardingProps()} />);

    expect(screen.queryByText(/skip setup/i)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    expect(screen.getByText("Swift")).toBeVisible();
    expect(screen.getByText("Balanced")).toBeVisible();
    expect(screen.getByText("Faithful")).toBeVisible();
    expect(screen.queryByText("Medium")).not.toBeInTheDocument();
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
    await user.click(screen.getByRole("button", { name: /test shortcut/i }));
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

    expect(screen.getByRole("button", { name: /listening for keys/i })).toBeDisabled();
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
});
