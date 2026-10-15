import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DeleteDialog } from "./DeleteDialog";
import { NameDialog, validateName } from "./NameDialog";

describe("NameDialog", () => {
  it("validates like the core", () => {
    expect(validateName("")).toBe("Bitte einen Namen eingeben.");
    expect(validateName("a/b")).toBe("Der Name darf kein / enthalten.");
    expect(validateName("..")).toBe("„.“ und „..“ sind keine gültigen Namen.");
    expect(validateName("ok")).toBeNull();
  });

  it("shows AlreadyExists from the core at the field", async () => {
    const submit = vi.fn(async () => {
      throw { type: "fs", kind: "alreadyExists", path: "/x/b", message: "exists" };
    });
    render(<NameDialog mode="rename" initial="a.txt" onSubmit={submit} onClose={() => {}} />);
    const field = screen.getByRole("textbox", { name: "Name" });
    await userEvent.clear(field);
    await userEvent.type(field, "b{Enter}");
    expect(await screen.findByText("Dieser Name existiert bereits.")).toBeInTheDocument();
  });

  it("preselects the name without extension when renaming", () => {
    render(<NameDialog mode="rename" initial="report.final.pdf" onSubmit={async () => {}} onClose={() => {}} />);
    const field = screen.getByRole("textbox", { name: "Name" }) as HTMLInputElement;
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, "report.final".length]);
  });
});

describe("DeleteDialog", () => {
  it("is a red permanent confirmation with focus on cancel", () => {
    render(<DeleteDialog mode="permanent" names={["dir"]} folder onConfirm={() => {}} onClose={() => {}} />);
    expect(screen.getByRole("alertdialog", { name: "Endgültig löschen?" })).toBeInTheDocument();
    expect(screen.getByText("Der Ordner wird mitsamt Inhalt entfernt.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Abbrechen" })).toHaveFocus();
  });

  it("offers the trash for local deletes", async () => {
    const confirm = vi.fn();
    render(<DeleteDialog mode="trash" names={["a", "b"]} folder={false} onConfirm={confirm} onClose={() => {}} />);
    expect(screen.getByText("2 Einträge werden in den Papierkorb verschoben.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "In den Papierkorb" }));
    expect(confirm).toHaveBeenCalled();
  });
});
