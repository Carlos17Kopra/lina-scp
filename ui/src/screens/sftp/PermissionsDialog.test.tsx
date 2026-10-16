import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ChmodReport, Entry } from "../../ipc/api";
import { PermissionsDialog } from "./PermissionsDialog";

const entry = (name: string, mode: number | null, kind: Entry["kind"] = { type: "file" }): Entry => ({
  name,
  kind,
  size: 1536,
  mtime: 1_700_000_000,
  mode,
  owner: "deploy",
  nameLossy: false,
});
const ok: ChmodReport = { failed: 0, first: null };
const box = (name: string) => screen.getByRole("checkbox", { name });
const octal = () => screen.getByRole("textbox", { name: "Oktal" });
const apply = () => screen.getByRole("button", { name: "Übernehmen" });

describe("PermissionsDialog", () => {
  it("shows one entry's details and applies only the edited bits", async () => {
    const submit = vi.fn(async () => ok);
    const close = vi.fn();
    render(<PermissionsDialog entries={[entry("a.txt", 0o100644)]} onSubmit={submit} onClose={close} />);
    expect(screen.getByText("a.txt")).toBeInTheDocument();
    expect(screen.getByText("deploy")).toBeInTheDocument();
    expect(screen.getByText("1,5 KB")).toBeInTheDocument();
    expect(box("Besitzer: Schreiben")).toBeChecked();
    expect(box("Andere: Schreiben")).not.toBeChecked();
    expect(octal()).toHaveValue("0644");
    expect(apply()).toBeDisabled();
    expect(screen.queryByRole("checkbox", { name: /enthaltene/ })).not.toBeInTheDocument();
    await userEvent.click(box("Andere: Schreiben"));
    expect(octal()).toHaveValue("0646");
    await userEvent.click(apply());
    expect(submit).toHaveBeenCalledWith({ set: 0o002, clear: 0 }, false);
    expect(close).toHaveBeenCalled();
  });

  it("keeps bits that differ between entries unless they are clicked", async () => {
    const submit = vi.fn(async () => ok);
    render(<PermissionsDialog entries={[entry("a", 0o100644), entry("b", 0o100600)]} onSubmit={submit} onClose={() => {}} />);
    expect(screen.getByText("2 Einträge")).toBeInTheDocument();
    expect(box("Gruppe: Lesen")).toBePartiallyChecked();
    expect(octal()).toHaveValue("");
    await userEvent.click(box("Gruppe: Lesen"));
    expect(box("Gruppe: Lesen")).toBeChecked();
    expect(octal()).toHaveValue("");
    await userEvent.click(apply());
    expect(submit).toHaveBeenCalledWith({ set: 0o040, clear: 0 }, false);
  });

  it("starts a symlink mixed: its own mode says nothing about the target", () => {
    render(<PermissionsDialog entries={[entry("l", 0o120777, { type: "symlink", target: "file" })]} onSubmit={async () => ok} onClose={() => {}} />);
    expect(box("Andere: Schreiben")).toBePartiallyChecked();
    expect(octal()).toHaveValue("");
  });

  it("recursively changes only the edited bits, but propagates the whole mode when nothing was edited", async () => {
    const submit = vi.fn(async () => ok);
    render(<PermissionsDialog entries={[entry("d", 0o40755, { type: "dir" })]} onSubmit={submit} onClose={() => {}} />);
    await userEvent.click(screen.getByRole("checkbox", { name: "Auch für enthaltene Dateien und Ordner" }));
    await userEvent.click(box("Gruppe: Schreiben"));
    await userEvent.click(apply());
    expect(submit).toHaveBeenCalledWith({ set: 0o020, clear: 0 }, true);
  });

  it("treats an unknown mode as mixed", () => {
    render(<PermissionsDialog entries={[entry("a", null)]} onSubmit={async () => ok} onClose={() => {}} />);
    expect(box("Besitzer: Lesen")).toBePartiallyChecked();
    expect(octal()).toHaveValue("");
  });

  it("sets every bit from the octal field and rejects invalid input", async () => {
    const submit = vi.fn(async () => ok);
    render(<PermissionsDialog entries={[entry("a", 0o100600), entry("b", 0o100640)]} onSubmit={submit} onClose={() => {}} />);
    await userEvent.type(octal(), "9");
    expect(screen.getByText("Drei Oktalziffern, z. B. 0644")).toBeInTheDocument();
    expect(apply()).toBeDisabled();
    await userEvent.clear(octal());
    await userEvent.type(octal(), "755");
    expect(box("Andere: Ausführen")).toBeChecked();
    expect(box("Gruppe: Lesen")).toBeChecked();
    await userEvent.click(apply());
    expect(submit).toHaveBeenCalledWith({ set: 0o755, clear: 0o022 }, false);
  });

  it("offers recursion only with a folder and passes it on", async () => {
    const submit = vi.fn(async () => ok);
    render(<PermissionsDialog entries={[entry("d", 0o40755, { type: "dir" })]} onSubmit={submit} onClose={() => {}} />);
    await userEvent.click(screen.getByRole("checkbox", { name: "Auch für enthaltene Dateien und Ordner" }));
    expect(apply()).toBeEnabled();
    await userEvent.click(apply());
    expect(submit).toHaveBeenCalledWith({ set: 0o755, clear: 0o022 }, true);
  });

  it("stays open and names the first failure", async () => {
    const report: ChmodReport = {
      failed: 2,
      first: { path: "/srv/a", error: { type: "fs", kind: "permissionDenied", path: "/srv/a", message: "denied" } },
    };
    const close = vi.fn();
    render(<PermissionsDialog entries={[entry("a", 0o100644)]} onSubmit={async () => report} onClose={close} />);
    await userEvent.click(box("Andere: Lesen"));
    await userEvent.click(apply());
    expect(await screen.findByText("2 Einträge konnten nicht geändert werden. Zuerst: /srv/a (denied)")).toBeInTheDocument();
    expect(close).not.toHaveBeenCalled();
  });
});
