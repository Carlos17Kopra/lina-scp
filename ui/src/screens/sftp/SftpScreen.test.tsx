import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { useEditsStore } from "../../stores/edits";
import { useHostsStore } from "../../stores/hosts";
import { useNoticeStore } from "../../stores/notices";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { makeHost, makeSessionInfo } from "../../test/fixtures";
import { SftpScreen } from "./SftpScreen";

vi.mock("../../ipc/api");

const start = vi.fn(async () => undefined);

beforeEach(() => {
  start.mockClear();
  vi.mocked(api.localHome).mockResolvedValue("/l");
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [] });
  vi.mocked(api.sftpClose).mockResolvedValue([]);
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useEditsStore.setState(useEditsStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  useHostsStore.setState({
    ...useHostsStore.getInitialState(),
    hosts: [
      makeHost({ id: "h1", name: "web", user: "deploy", address: "web.example" }),
      makeHost({ id: "h2", name: "ssh-only", protocolSftp: false }),
    ],
  }, true);
});

const openTab = (tabId = "t1", sessionId = "s1") =>
  useSftpStore.getState().openTab(tabId, "h1", {
    sessionId, info: makeSessionInfo({ cipher: "chacha20-poly1305@openssh.com" }), version: 3,
    listing: { path: "/srv", entries: [] },
  });
/** t1 active, t2 in the background. */
const openTwoTabs = () => {
  openTab("t2", "s2");
  openTab("t1", "s1");
};

describe("SftpScreen", () => {
  it("offers only SFTP hosts when no session is open", async () => {
    render(<SftpScreen />);
    expect(screen.queryByText("ssh-only")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /web/ }));
    expect(start).toHaveBeenCalledWith("h1", "sftp");
  });

  it("shows both panes and the status line", () => {
    openTab();
    render(<SftpScreen />);
    expect(screen.getByRole("grid", { name: "Lokal" })).toBeInTheDocument();
    expect(screen.getByRole("grid", { name: "Remote" })).toBeInTheDocument();
    expect(screen.getByText("deploy@web.example · SFTP v3 · chacha20-poly1305@openssh.com")).toBeInTheDocument();
  });

  it("shows the lost banner and reconnects", async () => {
    openTab();
    useSftpStore.getState().onEvent("t1", "s1", { kind: "lost", error: { type: "network", kind: "other", message: "connection lost" } });
    render(<SftpScreen />);
    expect(screen.getByText("Verbindung verloren")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(start).toHaveBeenCalledWith("h1", "sftp", { sftpTabId: "t1" });
  });

  it("opens the new-folder dialog from the toolbar and creates the folder", async () => {
    openTab();
    vi.mocked(api.sftpMkdir).mockResolvedValue();
    vi.mocked(api.sftpList).mockResolvedValue({ path: "/srv", entries: [] });
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neuer Ordner" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "neu{Enter}");
    expect(api.sftpMkdir).toHaveBeenCalledWith("s1", "/srv", "neu");
  });

  it("opens a terminal to the same host", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(start).toHaveBeenCalledWith("h1", "ssh");
  });

  it("the new-session picker can be left back to the open tabs", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neue SFTP-Sitzung" }));
    expect(screen.getByRole("heading", { name: "Host für SFTP wählen" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(screen.getByRole("grid", { name: "Remote" })).toBeInTheDocument();
  });

  it("closes a tab", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "web schließen" }));
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
  });

  it("the toolbar transfer button uploads from the local pane", async () => {
    vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [{ name: "a.txt", kind: { type: "file" }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false }] });
    vi.mocked(api.transferEnqueue).mockResolvedValue(1);
    openTab();
    await vi.waitFor(() => expect(useSftpStore.getState().tabs[0]!.panes.local.entries).toHaveLength(1));
    useSftpStore.getState().select("t1", "local", { selection: ["a.txt"], cursor: "a.txt", anchor: "a.txt" });
    useSftpStore.getState().setActivePane("t1", "local");
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Hochladen" }));
    expect(api.transferEnqueue).toHaveBeenCalledWith("s1", "upload", ["/l/a.txt"], "/srv");
  });

  it("transfer triggers are off while the session is not connected", () => {
    openTab();
    useSftpStore.getState().onEvent("t1", "s1", { kind: "lost", error: { type: "cancelled" } });
    render(<SftpScreen />);
    expect(screen.getByRole("button", { name: "Herunterladen" })).toBeDisabled();
  });

  it("shows the reconnect bar while reconnecting and calls sftpReconnectNow", async () => {
    openTab();
    useSftpStore.getState().onEvent("t1", "s1", { kind: "reconnecting", attempt: 1, maxAttempts: 8, retryInMs: 1000, reason: { type: "cancelled" } });
    render(<SftpScreen />);
    await userEvent.click(await screen.findByRole("button", { name: "Jetzt versuchen" }));
    expect(api.sftpReconnectNow).toHaveBeenCalledWith("s1");
    expect(screen.getByRole("button", { name: "Herunterladen" })).toBeDisabled();
  });

  it("asks before closing a tab with open transfers", async () => {
    openTab();
    useTransfersStore.getState().onEvent("t1", { kind: "job", job: { id: 1, batchId: 1, direction: "upload", source: "/a", target: "/b", rel: "a", kind: "file", size: 1, done: 0, state: "running", note: null, error: null, retryable: true } });
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "web schließen" }));
    expect(screen.getByRole("alertdialog", { name: "Übertragungen offen" })).toBeInTheDocument();
    expect(api.sftpClose).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
  });

  it("marks another tab that waits for a conflict decision and shows the dialog only in its own tab", () => {
    openTwoTabs();
    useTransfersStore.getState().onEvent("t2", { kind: "job", job: { id: 1, batchId: 1, direction: "upload", source: "/a", target: "/srv/a", rel: "a", kind: "file", size: 1, done: 0, state: "waitingDecision", note: null, error: null, retryable: true } });
    useTransfersStore.getState().onEvent("t2", { kind: "conflict", jobId: 1, source: { size: 1, mtime: 1 }, target: { size: 1, mtime: 1 } });
    render(<SftpScreen />);
    expect(screen.getByRole("tab", { name: /Wartet auf eine Entscheidung/ })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Datei existiert bereits" })).not.toBeInTheDocument();
  });

  it("edits remote files through the session and local files in place", async () => {
    vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [{ name: "a.txt", kind: { type: "file" }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false }] });
    vi.mocked(api.editOpen).mockResolvedValue(1);
    vi.mocked(api.localEdit).mockResolvedValue();
    useSftpStore.getState().openTab("t1", "h1", {
      sessionId: "s1", info: makeSessionInfo(), version: 3,
      listing: { path: "/srv", entries: [{ name: "a.txt", kind: { type: "file" }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false }] },
    });
    await vi.waitFor(() => expect(useSftpStore.getState().tabs[0]!.panes.local.entries).toHaveLength(1));
    render(<SftpScreen />);
    await userEvent.dblClick(within(screen.getByRole("grid", { name: "Remote" })).getByText("a.txt"));
    expect(api.editOpen).toHaveBeenCalledWith("s1", "/srv/a.txt", { kind: "default" });
    await userEvent.dblClick(within(screen.getByRole("grid", { name: "Lokal" })).getByText("a.txt"));
    expect(api.localEdit).toHaveBeenCalledWith("/l/a.txt", { kind: "default" });
  });

  it("opens a file with the app picked in the system chooser", async () => {
    vi.mocked(api.editOpen).mockResolvedValue(1);
    vi.mocked(api.openWithChoose).mockResolvedValueOnce(null).mockResolvedValueOnce({ id: "gimp.desktop", remember: true });
    useSftpStore.getState().openTab("t1", "h1", {
      sessionId: "s1", info: makeSessionInfo(), version: 3,
      listing: { path: "/srv", entries: [{ name: "a.png", kind: { type: "file" }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false }] },
    });
    render(<SftpScreen />);
    const openWith = async () => {
      fireEvent.contextMenu(within(screen.getByRole("grid", { name: "Remote" })).getByText("a.png"));
      await userEvent.click(screen.getByRole("menuitem", { name: "Öffnen mit …" }));
    };
    await openWith();
    expect(api.openWithChoose).toHaveBeenCalledWith("a.png", "Immer für diesen Dateityp verwenden");
    expect(api.editOpen).not.toHaveBeenCalled();
    await openWith();
    await vi.waitFor(() =>
      expect(api.editOpen).toHaveBeenCalledWith("s1", "/srv/a.png", { kind: "app", id: "gimp.desktop", remember: true }));
  });

  it("creates a new file from the toolbar", async () => {
    openTab();
    vi.mocked(api.sftpCreateFile).mockResolvedValue();
    vi.mocked(api.sftpList).mockResolvedValue({ path: "/srv", entries: [] });
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neue Datei" }));
    expect(screen.getByRole("dialog", { name: "Neue Datei" })).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "neu.txt{Enter}");
    expect(api.sftpCreateFile).toHaveBeenCalledWith("s1", "/srv", "neu.txt");
    expect(api.sftpList).toHaveBeenCalledWith("s1", "/srv");
  });

  it("changes permissions through the properties dialog and reloads", async () => {
    vi.mocked(api.sftpChmod).mockResolvedValue({ failed: 0, first: null });
    const entries = [{ name: "a.sh", kind: { type: "file" as const }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false }];
    vi.mocked(api.sftpList).mockResolvedValue({ path: "/srv", entries });
    useSftpStore.getState().openTab("t1", "h1", { sessionId: "s1", info: makeSessionInfo(), version: 3, listing: { path: "/srv", entries } });
    render(<SftpScreen />);
    fireEvent.contextMenu(within(screen.getByRole("grid", { name: "Remote" })).getByText("a.sh"));
    await userEvent.click(screen.getByRole("menuitem", { name: "Eigenschaften …" }));
    const dialog = screen.getByRole("dialog", { name: "Eigenschaften" });
    await userEvent.click(within(dialog).getByRole("checkbox", { name: "Besitzer: Ausführen" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Übernehmen" }));
    expect(api.sftpChmod).toHaveBeenCalledWith("s1", ["/srv/a.sh"], { set: 0o744, clear: 0o033 }, false);
    await vi.waitFor(() => expect(screen.queryByRole("dialog", { name: "Eigenschaften" })).not.toBeInTheDocument());
    expect(api.sftpList).toHaveBeenCalledWith("s1", "/srv");
  });

  it("the toolbar edit button is off on the remote side while not connected", () => {
    openTab();
    useSftpStore.getState().onEvent("t1", "s1", { kind: "lost", error: { type: "cancelled" } });
    useSftpStore.getState().setActivePane("t1", "remote");
    render(<SftpScreen />);
    expect(screen.getByRole("button", { name: "Bearbeiten" })).toBeDisabled();
  });

  it("counts unsaved edits when closing and names kept copies afterwards", async () => {
    openTab();
    vi.mocked(api.sftpClose).mockResolvedValue(["/c/x/a.conf"]);
    useEditsStore.getState().onEvent("t1", { kind: "file", file: { id: 1, name: "a.conf", remotePath: "/srv/a.conf", state: "changed", dirty: true, error: null } });
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "web schließen" }));
    expect(screen.getByRole("alertdialog", { name: "Übertragungen offen" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    await vi.waitFor(() => expect(useNoticeStore.getState().notices.map((n) => n.message)).toContain("Nicht hochgeladene Änderungen liegen unter /c/x/a.conf"));
  });

  it("marks another tab whose edit waits for a decision", () => {
    openTwoTabs();
    useEditsStore.getState().onEvent("t2", { kind: "file", file: { id: 1, name: "a.conf", remotePath: "/srv/a.conf", state: "conflict", dirty: true, error: null } });
    render(<SftpScreen />);
    expect(screen.getByRole("tab", { name: /Wartet auf eine Entscheidung/ })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Datei auf dem Server geändert" })).not.toBeInTheDocument();
  });
});
