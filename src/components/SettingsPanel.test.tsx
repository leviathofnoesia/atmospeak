import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { defaultSettings } from "../types/dictation";
import { SettingsPanel } from "./SettingsPanel";

function settingsProps(overrides: Record<string, unknown> = {}) {
  const settings = defaultSettings();
  return {
    settings,
    setSettings: vi.fn(),
    microphones: [
      {
        name: "Elgato Wave:3",
        isDefault: true,
        isSelected: true,
        available: true,
      },
    ],
    shortcutStatus: {
      registered: true,
      hotkey: settings.hotkey,
      paused: false,
      message: "Shortcut active.",
    },
    shortcutTest: { active: false, detected: false, message: "" },
    shortcutCapture: { arming: false, active: false, keys: [], message: "" },
    onTestShortcut: vi.fn(),
    onRecordShortcut: vi.fn(),
    onCancelShortcutCapture: vi.fn(),
    onShortcutChange: vi.fn(),
    onToggleShortcutsPaused: vi.fn(async () => undefined),
    onShowFloatingControl: vi.fn(async () => undefined),
    onResetDockPosition: vi.fn(async () => undefined),
    onRerunOnboarding: vi.fn(async () => undefined),
    onSave: vi.fn(async () => undefined),
    updateStatus: "idle" as const,
    updateResult: null,
    onCheckUpdates: vi.fn(async () => undefined),
    onInstallUpdate: vi.fn(async () => undefined),
    advanced: <div>Advanced diagnostics</div>,
    modelManagement: <div>Model management</div>,
    ...overrides,
  };
}

describe("SettingsPanel shortcut controls", () => {
  it("records arbitrary chords instead of limiting users to presets", () => {
    const props = settingsProps();
    render(<SettingsPanel {...props} />);

    fireEvent.click(screen.getByRole("button", { name: "Record any chord" }));
    expect(props.onRecordShortcut).toHaveBeenCalledOnce();
    expect(screen.getByLabelText("Shortcut keys")).toHaveTextContent("Ctrl");
    expect(screen.getByLabelText("Shortcut keys")).toHaveTextContent("Win");
  });

  it("lights every currently pressed key in real time", () => {
    render(
      <SettingsPanel
        {...settingsProps({
          shortcutCapture: {
            arming: false,
            active: true,
            keys: ["Ctrl", "Alt", "K"],
            message: "Keep holding.",
          },
        })}
      />,
    );

    for (const key of ["Ctrl", "Alt", "K"]) {
      expect(screen.getByText(key, { selector: "kbd" })).toHaveClass("is-down");
    }
  });

  it("describes and selects release-to-paste push-to-talk", () => {
    const props = settingsProps({
      settings: { ...defaultSettings(), mode: "toggle" as const },
    });
    render(<SettingsPanel {...props} />);

    fireEvent.click(screen.getByRole("button", { name: /^Hold/ }));
    expect(props.setSettings).toHaveBeenCalledWith(
      expect.objectContaining({ mode: "pushToTalk" }),
    );
    expect(
      screen.getByText("Press and hold to record. Releasing transcribes and pastes automatically."),
    ).toBeVisible();
  });

  it("keeps quick picks while allowing a custom recorded value", () => {
    const props = settingsProps();
    render(<SettingsPanel {...props} />);

    fireEvent.click(screen.getByRole("button", { name: "Ctrl+Shift+Space" }));
    expect(props.onShortcutChange).toHaveBeenCalledWith("Ctrl+Shift+Space");
  });
});
