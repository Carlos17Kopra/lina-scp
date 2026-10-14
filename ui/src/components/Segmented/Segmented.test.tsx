import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Segmented } from "./Segmented";

type Proto = "all" | "ssh" | "sftp";
const options = [
  { value: "all", label: "Alle" },
  { value: "ssh", label: "SSH" },
  { value: "sftp", label: "SFTP", disabled: true },
] as const;

function Harness({ onChange }: { onChange: (v: Proto) => void }) {
  const [value, setValue] = useState<Proto>("all");
  return (
    <Segmented
      label="Protokoll filtern"
      value={value}
      options={options}
      onChange={(v) => {
        setValue(v);
        onChange(v);
      }}
    />
  );
}

describe("Segmented", () => {
  it("is a radiogroup with one checked radio in the tab order", () => {
    render(<Harness onChange={() => undefined} />);
    expect(screen.getByRole("radiogroup", { name: "Protokoll filtern" })).toBeInTheDocument();
    const all = screen.getByRole("radio", { name: "Alle" });
    expect(all).toHaveAttribute("aria-checked", "true");
    expect(all).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "SSH" })).toHaveAttribute("tabindex", "-1");
  });

  it("selects on click", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await userEvent.click(screen.getByRole("radio", { name: "SSH" }));
    expect(onChange).toHaveBeenCalledWith("ssh");
    expect(screen.getByRole("radio", { name: "SSH" })).toHaveAttribute("aria-checked", "true");
  });

  it("moves selection and focus with arrow keys, skipping disabled options", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    screen.getByRole("radio", { name: "Alle" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("ssh");
    expect(screen.getByRole("radio", { name: "SSH" })).toHaveFocus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("all");
    expect(screen.getByRole("radio", { name: "Alle" })).toHaveFocus();
  });
});
