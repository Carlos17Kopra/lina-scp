import { render, screen, within } from "@testing-library/react";
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

beforeEach(() => {
  onCommand.mockClear();
  vi.mocked(api.localHome).mockResolvedValue("/l");
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [] });
  vi.mocked(api.sftpList).mockImplementation(async (_s, path) => ({ path, entries: [f("inner")] }));
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useSftpStore.getState().openTab("t1", "h1", {
    sessionId: "s1", info: makeSessionInfo(), version: 3,
    listing: { path: "/srv/www", entries: [f("b.txt"), f("A-dir", "dir"), f(".env"), f("bad?", "file", { nameLossy: true })] },
  });
});

const pane = () => useSftpStore.getState().tabs[0]!.panes.remote;
const rowNames = () => screen.getAllByRole("row").slice(1).map((r) => within(r).getAllByRole("gridcell")[0]!.textContent);

describe("FilePane", () => {
  it("lists .. and folders first, hides dot files", () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    expect(rowNames()).toEqual(["..", "A-dir", "b.txt", "bad?"]);
  });

  it("opens folders with Enter and goes up with Backspace", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    const grid = screen.getByRole("grid");
    grid.focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(api.sftpList).toHaveBeenCalledWith("s1", "/srv/www/A-dir");
    await userEvent.keyboard("{Backspace}");
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/srv/www");
  });

  it("selects with Shift+arrows and Ctrl+A, and sends commands for the targets", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
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
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{End}{F2}");
    expect(onCommand).not.toHaveBeenCalled();
    await userEvent.keyboard("{ArrowUp}{F2}");
    expect(onCommand).toHaveBeenLastCalledWith("rename", ["b.txt"]);
  });

  it("toggles hidden files with Ctrl+H and jumps by typing", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}h{/Control}");
    expect(rowNames()).toContain(".env");
    await userEvent.keyboard("b");
    expect(pane().cursor).toBe("b.txt");
  });

  it("Tab moves focus to the other pane's grid", async () => {
    render(
      <>
        <FilePane tabId="t1" side="local" onCommand={onCommand} />
        <FilePane tabId="t1" side="remote" onCommand={onCommand} />
      </>,
    );
    screen.getByRole("grid", { name: "Remote" }).focus();
    await userEvent.keyboard("{Tab}");
    expect(screen.getByRole("grid", { name: "Lokal" })).toHaveFocus();
    expect(useSftpStore.getState().tabs[0]!.active).toBe("local");
  });

  it("sorts by clicking a column header", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await userEvent.click(screen.getByRole("columnheader", { name: /Name/ }));
    expect(pane().sort).toEqual({ key: "name", dir: "desc" });
  });

  it("navigates via breadcrumbs and the path field", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
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
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
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
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await useSftpStore.getState().navigate("t1", "remote", "/root");
    expect(await screen.findByRole("alert")).toHaveTextContent("Keine Berechtigung");
  });

  it("tells the user to delete permanently when the trash is on another drive", async () => {
    render(<FilePane tabId="t1" side="local" onCommand={onCommand} />);
    await vi.waitFor(() => expect(useSftpStore.getState().tabs[0]!.panes.local.path).toBe("/l"));
    vi.mocked(api.localTrash).mockRejectedValueOnce({ type: "fs", kind: "crossDevice", path: "/l/x", message: "EXDEV" });
    await useSftpStore.getState().remove("t1", "local", ["x"], "trash");
    expect(await screen.findByRole("alert")).toHaveTextContent("Umschalt+Entf");
  });

  it("right-click selects the row and opens a context menu", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await userEvent.pointer({ keys: "[MouseRight]", target: screen.getByText("b.txt") });
    expect(pane().selection).toEqual(["b.txt"]);
    await userEvent.click(screen.getByRole("menuitem", { name: "Löschen" }));
    expect(onCommand).toHaveBeenLastCalledWith("delete", ["b.txt"]);
  });
});
