import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Entry } from "../../ipc/api";
import { useSftpStore } from "../../stores/sftp";
import { makeSessionInfo } from "../../test/fixtures";
import { FilePane } from "./FilePane";

vi.mock("../../ipc/api");

const f = (name: string, type: "file" | "dir" = "file", extra: Partial<Entry> = {}): Entry => ({
  name, kind: { type }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false, ...extra,
});

const onCommand = vi.fn();
const onTransfer = vi.fn();

beforeEach(() => {
  onCommand.mockClear();
  onTransfer.mockClear();
  vi.mocked(api.localHome).mockResolvedValue("/l");
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [f("a.txt")] });
  vi.mocked(api.sftpList).mockImplementation(async (_s, path) => ({ path, entries: [f("inner")] }));
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useSftpStore.getState().openTab("t1", "h1", {
    sessionId: "s1", info: makeSessionInfo(), version: 3,
    listing: { path: "/srv/www", entries: [f("b.txt"), f("A-dir", "dir"), f(".env"), f("bad?", "file", { nameLossy: true })] },
  });
});

const pane = () => useSftpStore.getState().tabs[0]!.panes.remote;
const localLoaded = () => vi.waitFor(() => expect(useSftpStore.getState().tabs[0]!.panes.local.entries).toHaveLength(1));
const renderBothPanes = () =>
  render(
    <>
      <FilePane tabId="t1" side="local" onCommand={onCommand} onTransfer={onTransfer} />
      <FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />
    </>,
  );
const rowNames = () => screen.getAllByRole("row").slice(1).map((r) => within(r).getAllByRole("gridcell")[0]!.textContent);

describe("FilePane", () => {
  it("lists .. and folders first, hides dot files", () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    expect(rowNames()).toEqual(["..", "A-dir", "b.txt", "bad?"]);
  });

  it("opens folders with Enter and goes up with Backspace", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    const grid = screen.getByRole("grid");
    grid.focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(api.sftpList).toHaveBeenCalledWith("s1", "/srv/www/A-dir");
    await userEvent.keyboard("{Backspace}");
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/srv/www");
  });

  it("selects with Shift+arrows and Ctrl+A, and sends commands for the targets", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Shift>}{ArrowDown}{/Shift}");
    expect(pane().selection).toEqual(["A-dir", "b.txt"]);
    await userEvent.keyboard("{Delete}");
    expect(onCommand).toHaveBeenLastCalledWith("delete", ["A-dir", "b.txt"]);
    await userEvent.keyboard("{Control>}a{/Control}");
    expect(pane().selection).toEqual(["A-dir", "b.txt"]);
    await userEvent.keyboard("{F7}");
    expect(onCommand).toHaveBeenLastCalledWith("newFolder", []);
  });

  it("F2 renames exactly one entry and ignores lossy names", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{End}{F2}");
    expect(onCommand).not.toHaveBeenCalled();
    await userEvent.keyboard("{ArrowUp}{F2}");
    expect(onCommand).toHaveBeenLastCalledWith("rename", ["b.txt"]);
  });

  it("toggles hidden files with Ctrl+H and jumps by typing", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}h{/Control}");
    expect(rowNames()).toContain(".env");
    await userEvent.keyboard("b");
    expect(pane().cursor).toBe("b.txt");
  });

  it("Tab moves focus to the other pane's grid", async () => {
    render(
      <>
        <FilePane tabId="t1" side="local" onCommand={onCommand} onTransfer={onTransfer} />
        <FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />
      </>,
    );
    screen.getByRole("grid", { name: "Remote" }).focus();
    await userEvent.keyboard("{Tab}");
    expect(screen.getByRole("grid", { name: "Lokal" })).toHaveFocus();
    expect(useSftpStore.getState().tabs[0]!.active).toBe("local");
  });

  it("sorts by clicking a column header", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    await userEvent.click(screen.getByRole("columnheader", { name: /Name/ }));
    expect(pane().sort).toEqual({ key: "name", dir: "desc" });
  });

  it("navigates via breadcrumbs and the path field", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    await userEvent.click(screen.getByRole("button", { name: "srv" }));
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/srv");
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}l{/Control}");
    const field = screen.getByRole("textbox", { name: "Pfad" });
    await userEvent.clear(field);
    await userEvent.type(field, "/etc{Enter}");
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/etc");
  });

  it("keys inside the path field stay with the field", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}l{/Control}");
    const field = screen.getByRole("textbox", { name: "Pfad" });
    vi.mocked(api.sftpList).mockClear();
    await userEvent.type(field, "{Backspace}{F2}{Delete}b");
    expect(onCommand).not.toHaveBeenCalled();
    expect(api.sftpList).not.toHaveBeenCalled();
  });

  it("shows a navigation error above the table", async () => {
    vi.mocked(api.sftpList).mockRejectedValueOnce({ type: "fs", kind: "permissionDenied", path: "/root", message: "denied" });
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    await useSftpStore.getState().navigate("t1", "remote", "/root");
    expect(await screen.findByRole("alert")).toHaveTextContent("Keine Berechtigung");
  });

  it("tells the user to delete permanently when the trash is on another drive", async () => {
    render(<FilePane tabId="t1" side="local" onCommand={onCommand} onTransfer={onTransfer} />);
    await vi.waitFor(() => expect(useSftpStore.getState().tabs[0]!.panes.local.path).toBe("/l"));
    vi.mocked(api.localTrash).mockRejectedValueOnce({ type: "fs", kind: "crossDevice", path: "/l/x", message: "EXDEV" });
    await useSftpStore.getState().remove("t1", "local", ["x"], "trash");
    expect(await screen.findByRole("alert")).toHaveTextContent("Umschalt+Entf");
  });

  it("right-click selects the row and opens a context menu", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    await userEvent.pointer({ keys: "[MouseRight]", target: screen.getByText("b.txt") });
    expect(pane().selection).toEqual(["b.txt"]);
    await userEvent.click(screen.getByRole("menuitem", { name: "Löschen" }));
    expect(onCommand).toHaveBeenLastCalledWith("delete", ["b.txt"]);
  });

  it("F5 transfers the selection to the other pane", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}{F5}");
    expect(onTransfer).toHaveBeenCalledWith(["b.txt"], "local", null);
  });

  it("offers Hochladen in the local and Herunterladen in the remote context menu", async () => {
    renderBothPanes();
    await localLoaded();
    await userEvent.pointer({ keys: "[MouseRight]", target: screen.getByText("a.txt") });
    await userEvent.click(screen.getByRole("menuitem", { name: "Hochladen" }));
    expect(onTransfer).toHaveBeenLastCalledWith(["a.txt"], "remote", null);
    await userEvent.pointer({ keys: "[MouseRight]", target: screen.getByText("b.txt") });
    await userEvent.click(screen.getByRole("menuitem", { name: "Herunterladen" }));
    expect(onTransfer).toHaveBeenLastCalledWith(["b.txt"], "local", null);
  });

  it("dragging rows onto the other pane's folder transfers into that folder", async () => {
    renderBothPanes();
    await localLoaded();
    const row = within(screen.getByRole("region", { name: "Lokal" })).getByText("a.txt");
    const target = within(screen.getByRole("region", { name: "Remote" })).getByText("A-dir");
    document.elementFromPoint = vi.fn(() => target);
    fireEvent.pointerDown(row, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(document, { clientX: 300, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(document, { clientX: 300, clientY: 40, pointerId: 1 });
    expect(onTransfer).toHaveBeenCalledWith(["a.txt"], "remote", "A-dir");
  });

  it("a drop into the same pane or a short click is not a transfer", async () => {
    renderBothPanes();
    await localLoaded();
    const row = within(screen.getByRole("region", { name: "Lokal" })).getByText("a.txt");
    document.elementFromPoint = vi.fn(() => row);
    fireEvent.pointerDown(row, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(document, { clientX: 300, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(document, { clientX: 300, clientY: 40, pointerId: 1 });
    fireEvent.pointerDown(row, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerUp(document, { clientX: 12, clientY: 11, pointerId: 1 });
    expect(onTransfer).not.toHaveBeenCalled();
  });

  it("Ctrl+B toggles the bookmark of the current folder, per side", async () => {
    vi.mocked(api.bookmarksList).mockResolvedValue([]);
    vi.mocked(api.bookmarkAdd).mockResolvedValue({ id: "b1", hostId: "h1", path: "/srv/www", position: 1, createdAt: 0 });
    renderBothPanes();
    await localLoaded();
    screen.getByRole("grid", { name: "Remote" }).focus();
    await userEvent.keyboard("{Control>}b{/Control}");
    expect(api.bookmarkAdd).toHaveBeenCalledWith("h1", "/srv/www");
    screen.getByRole("grid", { name: "Lokal" }).focus();
    await userEvent.keyboard("{Control>}b{/Control}");
    expect(api.bookmarkAdd).toHaveBeenLastCalledWith(null, "/l");
  });

  it("edits files with F4, Enter, double click and the menu; folders still open", async () => {
    const onEdit = vi.fn();
    useSftpStore.setState(useSftpStore.getInitialState(), true);
    useSftpStore.getState().openTab("t1", "h1", {
      sessionId: "s1", info: makeSessionInfo(), version: 3,
      listing: { path: "/srv", entries: [
        f("b.txt"), f("dir", "dir"), f("bad?", "file", { nameLossy: true }),
        f("link-f", "file", { kind: { type: "symlink", target: "file" } }), f("link-d", "file", { kind: { type: "symlink", target: "dir" } }),
      ] },
    });
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} onEdit={onEdit} />);
    const grid = screen.getByRole("grid");
    grid.focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}{ArrowDown}");
    expect(pane().cursor).toBe("b.txt");
    await userEvent.keyboard("{F4}");
    await userEvent.keyboard("{Enter}");
    expect(onEdit.mock.calls).toEqual([["b.txt", "editor"], ["b.txt", "default"]]);
    await userEvent.dblClick(screen.getByText("link-f"));
    expect(onEdit).toHaveBeenLastCalledWith("link-f", "default");
    await userEvent.dblClick(screen.getByText("bad?"));
    expect(onEdit).toHaveBeenCalledTimes(3);
    await userEvent.dblClick(screen.getByText("link-d"));
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/srv/link-d");
    expect(onEdit).toHaveBeenCalledTimes(3);
  });

  it("offers Bearbeiten in the context menu", async () => {
    const onEdit = vi.fn();
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} onEdit={onEdit} />);
    fireEvent.contextMenu(screen.getByText("b.txt"));
    await userEvent.click(screen.getByRole("menuitem", { name: "Bearbeiten" }));
    expect(onEdit).toHaveBeenCalledWith("b.txt", "editor");
    fireEvent.contextMenu(screen.getByText("b.txt"));
    await userEvent.click(screen.getByRole("menuitem", { name: "Öffnen mit …" }));
    expect(onEdit).toHaveBeenLastCalledWith("b.txt", "choose");
  });

  it("creates a new file with Shift+F4 and from the menu", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} onTransfer={onTransfer} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Shift>}{F4}{/Shift}");
    expect(onCommand).toHaveBeenLastCalledWith("newFile", []);
    onCommand.mockClear();
    fireEvent.contextMenu(screen.getByText("b.txt"));
    await userEvent.click(screen.getByRole("menuitem", { name: "Neue Datei" }));
    expect(onCommand).toHaveBeenLastCalledWith("newFile", []);
  });
});
