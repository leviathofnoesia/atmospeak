import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LicenseStatus } from "../types/dictation";
import { freeLicenseStatus } from "../types/dictation";
import { LicensePanel } from "./LicensePanel";

const activateLicense = vi.fn();
const deactivateLicense = vi.fn();
const getLicenseStatus = vi.fn();

vi.mock("../lib/api", () => ({
  activateLicense: (key: string) => activateLicense(key),
  deactivateLicense: () => deactivateLicense(),
  getLicenseStatus: () => getLicenseStatus(),
}));

function proStatus(overrides: Partial<LicenseStatus> = {}): LicenseStatus {
  return {
    ...freeLicenseStatus("2026-07-29"),
    tier: "pro",
    activated: true,
    inUpdateWindow: true,
    licenseId: "338964612494",
    issuedAt: "2026-07-29",
    updatesUntil: "2027-07-29",
    seats: 1,
    features: ["compliancePack", "mcpServer"],
    ...overrides,
  };
}

describe("LicensePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getLicenseStatus.mockResolvedValue(freeLicenseStatus("2026-07-29"));
    deactivateLicense.mockResolvedValue(freeLicenseStatus("2026-07-29"));
  });

  it("shows the free tier and an activation field when no licence is stored", async () => {
    render(<LicensePanel />);

    expect(await screen.findByText("Free")).toBeTruthy();
    expect(screen.getByPlaceholderText("ATMO-...")).toBeTruthy();
  });

  it("states plainly that activation makes no network request", async () => {
    render(<LicensePanel />);

    expect(await screen.findByText(/contacts no server/i)).toBeTruthy();
  });

  it("lists unlocked features after a successful activation", async () => {
    activateLicense.mockResolvedValue(proStatus());
    render(<LicensePanel />);

    fireEvent.change(await screen.findByPlaceholderText("ATMO-..."), {
      target: { value: "ATMO-VALID-KEY" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Activate" }));

    await waitFor(() => expect(screen.getByText(/Pro licence activated/i)).toBeTruthy());
    expect(screen.getByText(/MCP server for coding agents/i)).toBeTruthy();
    expect(screen.getByText(/Includes updates through 2027-07-29/i)).toBeTruthy();
  });

  it("surfaces a rejected key as an error without crashing", async () => {
    activateLicense.mockRejectedValue(new Error("licence key signature is not valid"));
    render(<LicensePanel />);

    fireEvent.change(await screen.findByPlaceholderText("ATMO-..."), {
      target: { value: "ATMO-FORGED" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Activate" }));

    await waitFor(() =>
      expect(screen.getByText("licence key signature is not valid")).toBeTruthy(),
    );
    // Still on the free tier, and the activation field is still available.
    expect(screen.getByPlaceholderText("ATMO-...")).toBeTruthy();
  });

  it("explains an out-of-window build without calling the licence invalid", async () => {
    getLicenseStatus.mockResolvedValue(
      proStatus({ inUpdateWindow: false, features: [], buildReleasedOn: "2028-01-05" }),
    );
    render(<LicensePanel />);

    expect(await screen.findByText(/Update window ended 2027-07-29/i)).toBeTruthy();
    // The purchase is still acknowledged.
    expect(screen.getByText("Pro")).toBeTruthy();
  });

  it("falls back to the free tier when the status lookup fails", async () => {
    getLicenseStatus.mockRejectedValue(new Error("keyring unavailable"));
    render(<LicensePanel />);

    expect(await screen.findByText("Free")).toBeTruthy();
  });

  it("removes a stored licence on request", async () => {
    getLicenseStatus.mockResolvedValue(proStatus());
    render(<LicensePanel />);

    fireEvent.click(
      await screen.findByRole("button", { name: /Remove licence from this machine/i }),
    );

    await waitFor(() => expect(deactivateLicense).toHaveBeenCalled());
    expect(await screen.findByText("Free")).toBeTruthy();
  });
});
