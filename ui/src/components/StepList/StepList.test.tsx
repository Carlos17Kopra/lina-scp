import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StepList } from "./StepList";

describe("StepList", () => {
  it("renders each state with an accessible status", () => {
    render(
      <StepList
        steps={[
          { id: "tcp", label: "Verbinden", state: "done", meta: "10.0.1.12:22" },
          { id: "hostKey", label: "Host-Key prüfen", state: "failed" },
          { id: "auth", label: "Anmelden", state: "active", detail: <input aria-label="Code" /> },
          { id: "shell", label: "Shell starten", state: "pending" },
        ]}
      />,
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(4);
    expect(within(items[0]!).getByText(/erledigt/)).toBeInTheDocument();
    expect(within(items[0]!).getByText("10.0.1.12:22")).toBeInTheDocument();
    expect(within(items[1]!).getByText(/fehlgeschlagen/)).toBeInTheDocument();
    expect(items[2]).toHaveAttribute("aria-current", "step");
    expect(within(items[2]!).getByRole("textbox", { name: "Code" })).toBeInTheDocument();
    expect(within(items[3]!).getByText(/ausstehend/)).toBeInTheDocument();
    expect(items[3]).not.toHaveAttribute("aria-current");
  });
});
