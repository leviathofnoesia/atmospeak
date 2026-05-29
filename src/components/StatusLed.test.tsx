import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StatusLed } from "./StatusLed";

describe("StatusLed", () => {
  it("renders the supplied label", () => {
    render(<StatusLed tone="good" label="Offline engine ready" />);

    expect(screen.getByText("Offline engine ready")).toBeInTheDocument();
  });
});
