import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TextField } from "./TextField";

describe("TextField", () => {
  it("associates the label with the input", () => {
    render(<TextField label="Adresse" value="" onChange={() => undefined} placeholder="10.0.1.12" />);
    const input = screen.getByLabelText("Adresse");
    expect(input).toHaveAttribute("type", "text");
    expect(input).toHaveAttribute("placeholder", "10.0.1.12");
    expect(input).not.toHaveAttribute("aria-invalid");
  });

  it("reports changes as plain strings", async () => {
    const onChange = vi.fn();
    render(<TextField label="Name" value="" onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("Name"), "w");
    expect(onChange).toHaveBeenCalledWith("w");
  });

  it("renders the error with aria-invalid and aria-describedby", () => {
    render(
      <TextField label="Port" value="0" onChange={() => undefined} hint="Standard: 22" error="Port muss zwischen 1 und 65535 liegen" />,
    );
    const input = screen.getByLabelText("Port");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input.getAttribute("aria-describedby")).toMatch(/-hint .*-error$/);
    expect(input).toHaveAccessibleDescription("Standard: 22 Port muss zwischen 1 und 65535 liegen");
  });
});
