import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ATMOSPEAK_PRO_BUY_URL, ATMOSPEAK_PRO_INFO_URL } from "../lib/api";
import { ProUpgradePrompt } from "./ProUpgradePrompt";

const mockOpenExternalUrl = vi.fn();

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    openExternalUrl: (...args: Parameters<typeof actual.openExternalUrl>) =>
      mockOpenExternalUrl(...args),
  };
});

describe("ProUpgradePrompt", () => {
  beforeEach(() => {
    mockOpenExternalUrl.mockReset();
    mockOpenExternalUrl.mockResolvedValue(undefined);
  });

  it("opens the Polar checkout portal when upgrading from workspace history", async () => {
    render(<ProUpgradePrompt variant="workspace" />);

    expect(screen.getByText("$69 once · Polar")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Upgrade to Pro — $69 once" }));

    await waitFor(() => {
      expect(mockOpenExternalUrl).toHaveBeenCalledWith(ATMOSPEAK_PRO_BUY_URL);
    });
  });

  it("opens comparison details cleanly and renders an inline URL fallback on error", async () => {
    mockOpenExternalUrl.mockRejectedValueOnce(new Error("opener failed"));
    render(<ProUpgradePrompt variant="privacy" />);

    fireEvent.click(screen.getByRole("button", { name: "Compare Free vs Pro" }));

    await waitFor(() => {
      expect(mockOpenExternalUrl).toHaveBeenCalledWith(ATMOSPEAK_PRO_INFO_URL);
      expect(screen.getByRole("status")).toHaveTextContent(ATMOSPEAK_PRO_INFO_URL);
    });
  });
});
