import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Pencil } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("uses the label as accessible name and tooltip", async () => {
    const onClick = vi.fn();
    render(<IconButton label="Host bearbeiten" icon={Pencil} onClick={onClick} />);
    const button = screen.getByRole("button", { name: "Host bearbeiten" });
    expect(button).toHaveAttribute("title", "Host bearbeiten");
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("allows overriding the tooltip", () => {
    render(<IconButton label="Snippets" icon={Pencil} title="Snippets – später" />);
    expect(screen.getByRole("button", { name: "Snippets" })).toHaveAttribute("title", "Snippets – später");
  });
});
