import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Button } from "../Button/Button";
import { TextField } from "../TextField/TextField";
import { Dialog } from "./Dialog";

function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Öffnen
      </button>
      <Dialog
        title="Einstellungen"
        open={open}
        onClose={() => setOpen(false)}
        footer={<Button onClick={() => setOpen(false)}>OK</Button>}
      >
        <TextField label="Name" value="" onChange={() => undefined} />
      </Dialog>
    </>
  );
}

async function openDialog() {
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: "Öffnen" }));
  return user;
}

describe("Dialog", () => {
  it("is a labelled modal dialog that focuses the first field", async () => {
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Einstellungen" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByLabelText("Name")).toHaveFocus();
  });

  it("traps Tab and Shift+Tab inside the dialog", async () => {
    const user = await openDialog();
    const ok = screen.getByRole("button", { name: "OK" });
    const close = screen.getByRole("button", { name: "Schließen" });
    await user.tab();
    expect(ok).toHaveFocus();
    await user.tab();
    expect(close).toHaveFocus();
    await user.tab({ shift: true });
    expect(ok).toHaveFocus();
  });

  it("closes on Escape and restores focus to the trigger", async () => {
    const user = await openDialog();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Öffnen" })).toHaveFocus();
  });

  it("closes via the close button", async () => {
    const user = await openDialog();
    await user.click(screen.getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps focus in the dialog on backdrop mousedown", async () => {
    const user = await openDialog();
    const backdrop = screen.getByRole("dialog").parentElement as HTMLElement;
    await user.pointer({ keys: "[MouseLeft>]", target: backdrop });
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("returns focus into the dialog when it lands outside, and Escape still closes", async () => {
    const user = userEvent.setup();
    render(
      <>
        <button type="button">Daneben</button>
        <Harness />
      </>,
    );
    await user.click(screen.getByRole("button", { name: "Öffnen" }));
    act(() => {
      screen.getByRole("button", { name: "Daneben" }).focus();
    });
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renders nothing when closed", () => {
    render(
      <Dialog title="X" open={false} onClose={() => undefined}>
        body
      </Dialog>,
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
