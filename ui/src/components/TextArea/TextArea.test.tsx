import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TextArea } from "./TextArea";

describe("TextArea", () => {
  it("is labelled and reports changes", async () => {
    const onChange = vi.fn();
    render(<TextArea label="Notizen" value="" onChange={onChange} hint="Optional" />);
    const area = screen.getByLabelText("Notizen");
    expect(area.tagName).toBe("TEXTAREA");
    expect(area).toHaveAccessibleDescription("Optional");
    await userEvent.type(area, "x");
    expect(onChange).toHaveBeenCalledWith("x");
  });

  it("shows an error", () => {
    render(<TextArea label="Umgebungsvariablen" value="kaputt" onChange={() => undefined} error="Zeile 1: Format KEY=VALUE" />);
    expect(screen.getByLabelText("Umgebungsvariablen")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Zeile 1: Format KEY=VALUE")).toBeInTheDocument();
  });
});
