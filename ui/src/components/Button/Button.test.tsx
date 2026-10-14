import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Plus } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";

describe("Button", () => {
  it.each(["primary", "secondary", "ghost", "danger", "inverted"] as const)("renders the %s variant", (variant) => {
    render(<Button variant={variant}>Speichern</Button>);
    const button = screen.getByRole("button", { name: "Speichern" });
    expect(button).toHaveAttribute("data-variant", variant);
    expect(button).toHaveAttribute("type", "button");
  });

  it("defaults to the secondary variant", () => {
    render(<Button>Abbrechen</Button>);
    expect(screen.getByRole("button", { name: "Abbrechen" })).toHaveAttribute("data-variant", "secondary");
  });

  it("renders a decorative icon and forwards clicks", async () => {
    const onClick = vi.fn();
    render(
      <Button variant="primary" icon={Plus} onClick={onClick}>
        Neuer Host
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Neuer Host" });
    expect(button.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("does not fire when disabled", async () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Speichern
      </Button>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(onClick).not.toHaveBeenCalled();
  });
});
