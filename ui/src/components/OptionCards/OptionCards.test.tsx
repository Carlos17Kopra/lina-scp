import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { OptionCards } from "./OptionCards";

const OPTIONS = [
  { value: "integrate", label: "Integrieren", hint: "Nur lesen" },
  { value: "mirror", label: "Spiegeln", hint: "Zusätzlich schreiben" },
] as const;

function Harness() {
  const [value, setValue] = useState<"integrate" | "mirror">("integrate");
  return <OptionCards label="ssh-Modus" value={value} options={OPTIONS} onChange={setValue} />;
}

describe("OptionCards", () => {
  it("is a labelled radiogroup with hints as descriptions", () => {
    render(<Harness />);
    expect(screen.getByRole("radiogroup", { name: "ssh-Modus" })).toBeInTheDocument();
    const integrate = screen.getByRole("radio", { name: "Integrieren" });
    expect(integrate).toHaveAttribute("aria-checked", "true");
    expect(integrate).toHaveAccessibleDescription("Nur lesen");
    expect(integrate).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "Spiegeln" })).toHaveAttribute("tabindex", "-1");
  });

  it("selects by click and arrow keys", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    expect(screen.getByRole("radio", { name: "Spiegeln" })).toHaveAttribute("aria-checked", "true");
    fireEvent.keyDown(screen.getByRole("radio", { name: "Spiegeln" }), { key: "ArrowRight" });
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveFocus();
  });
});
