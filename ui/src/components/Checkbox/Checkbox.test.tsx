import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Checkbox } from "./Checkbox";

describe("Checkbox", () => {
  it("toggles via its label", async () => {
    const onChange = vi.fn();
    render(<Checkbox label="Geheimnisse einschließen" checked={false} onChange={onChange} />);
    await userEvent.click(screen.getByText("Geheimnisse einschließen"));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("describes itself with the hint and respects disabled", async () => {
    const onChange = vi.fn();
    render(<Checkbox label="Im Profil speichern" checked onChange={onChange} hint="Im Schlüsselbund" disabled />);
    const box = screen.getByRole("checkbox", { name: "Im Profil speichern" });
    expect(box).toBeChecked();
    expect(box).toHaveAccessibleDescription("Im Schlüsselbund");
    await userEvent.click(box);
    expect(onChange).not.toHaveBeenCalled();
  });
});
