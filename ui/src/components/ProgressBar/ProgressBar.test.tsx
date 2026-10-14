import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProgressBar } from "./ProgressBar";

describe("ProgressBar", () => {
  it("exposes the value in percent", () => {
    render(<ProgressBar value={0.4} label="Verbindungsaufbau" />);
    const bar = screen.getByRole("progressbar", { name: "Verbindungsaufbau" });
    expect(bar).toHaveAttribute("aria-valuenow", "40");
    expect(bar).toHaveAttribute("aria-valuemin", "0");
    expect(bar).toHaveAttribute("aria-valuemax", "100");
  });

  it("clamps out-of-range values", () => {
    render(<ProgressBar value={3} label="A" />);
    expect(screen.getByRole("progressbar", { name: "A" })).toHaveAttribute("aria-valuenow", "100");
  });
});
