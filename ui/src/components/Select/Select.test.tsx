import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Select } from "./Select";

const options = [
  { value: "auto", label: "Automatisch" },
  { value: "webgl", label: "WebGL" },
  { value: "dom", label: "DOM" },
] as const;

describe("Select", () => {
  it("renders a labelled native select and reports the chosen value", async () => {
    const onChange = vi.fn();
    render(<Select label="Terminal-Renderer" value="auto" options={options} onChange={onChange} />);
    const select = screen.getByLabelText("Terminal-Renderer");
    expect(select).toHaveValue("auto");
    await userEvent.selectOptions(select, "dom");
    expect(onChange).toHaveBeenCalledWith("dom");
  });

  it("marks errors", () => {
    render(<Select label="Gruppe" value="auto" options={options} onChange={() => undefined} error="Pflichtfeld" />);
    expect(screen.getByLabelText("Gruppe")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Gruppe")).toHaveAccessibleDescription("Pflichtfeld");
  });
});
