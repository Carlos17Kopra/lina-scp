import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Difference } from "../../ipc/api";
import { useNoticeStore } from "../../stores/notices";
import { SyncDialog } from "./SyncDialog";

vi.mock("../../ipc/api");

const f = (size: number, mtime: number) => ({ dir: false, size, mtime });
const DIFFS: Difference[] = [
  { rel: "index.html", status: "localNewer", local: f(10, 2000), remote: f(8, 1000) },
  { rel: "img", status: "onlyLocal", local: { dir: true, size: 0, mtime: 1 }, remote: null },
  { rel: "old.css", status: "onlyRemote", local: null, remote: f(3, 500) },
];

const props = { tabId: "t1", sessionId: "s1", hostId: "h1", localDir: "/l", remoteDir: "/srv" };

beforeEach(() => {
  localStorage.clear();
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  vi.mocked(api.syncCompare).mockResolvedValue(DIFFS);
  vi.mocked(api.syncEnqueue).mockResolvedValue([1]);
  vi.mocked(api.syncDelete).mockResolvedValue({ deleted: 1, skipped: 0, failed: 0, first: null });
  vi.mocked(api.syncCancel).mockResolvedValue();
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [] });
  vi.mocked(api.sftpList).mockResolvedValue({ path: "/srv", entries: [] });
});

const row = (rel: string) => screen.getByRole("row", { name: new RegExp(rel.replace(".", "\\.")) });

describe("SyncDialog", () => {
  it("compares on open and previews the upload", async () => {
    render(<SyncDialog {...props} onClose={() => {}} />);
    expect(screen.getByText("Vergleiche …")).toBeInTheDocument();
    expect(await screen.findByText("index.html")).toBeInTheDocument();
    expect(api.syncCompare).toHaveBeenCalledWith("s1", "/l", "/srv", []);
    expect(within(row("index.html")).getByText("Hochladen")).toBeInTheDocument();
    expect(within(row("old.css")).getByText("Übersprungen: nur remote")).toBeInTheDocument();
    expect(within(row("old.css")).getByRole("checkbox")).toBeDisabled();
    expect(screen.getByText("2 hochladen")).toBeInTheDocument();
  });

  it("announces remote deletes in red and runs uploads and deletes", async () => {
    const close = vi.fn();
    render(<SyncDialog {...props} onClose={close} />);
    await screen.findByText("index.html");
    await userEvent.click(screen.getByRole("checkbox", { name: "Auf der Zielseite löschen, was es nur dort gibt" }));
    expect(screen.getByText("1 Eintrag wird auf dem Server endgültig gelöscht.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "img übernehmen" }));
    expect(screen.getByText("1 hochladen, 1 auf dem Server löschen")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Synchronisieren" }));
    await vi.waitFor(() => expect(close).toHaveBeenCalled());
    expect(api.syncEnqueue).toHaveBeenCalledWith("s1", "upload", "/l", "/srv", [{ rel: "index.html", expected: { size: 8, mtime: 1000 } }]);
    expect(api.syncDelete).toHaveBeenCalledWith("s1", "remote", "/srv", [{ rel: "old.css", expected: { size: 3, mtime: 500 } }]);
  });

  it("downloads when the direction changes", async () => {
    render(<SyncDialog {...props} onClose={() => {}} />);
    await screen.findByText("index.html");
    await userEvent.click(screen.getByRole("radio", { name: "Herunterladen" }));
    expect(within(row("index.html")).getByText("Übersprungen: lokal neuer")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Synchronisieren" }));
    expect(api.syncEnqueue).toHaveBeenCalledWith("s1", "download", "/l", "/srv", [{ rel: "old.css", expected: null }]);
    expect(api.syncDelete).not.toHaveBeenCalled();
  });

  it("remembers the exclude patterns per host and compares again with them", async () => {
    const { unmount } = render(<SyncDialog {...props} onClose={() => {}} />);
    await screen.findByText("index.html");
    await userEvent.type(screen.getByRole("textbox", { name: "Ausschließen" }), ".git, *.tmp ,");
    await userEvent.click(screen.getByRole("button", { name: "Neu vergleichen" }));
    expect(api.syncCompare).toHaveBeenLastCalledWith("s1", "/l", "/srv", [".git", "*.tmp"]);
    unmount();
    render(<SyncDialog {...props} onClose={() => {}} />);
    expect(api.syncCompare).toHaveBeenLastCalledWith("s1", "/l", "/srv", [".git", "*.tmp"]);
    expect(screen.getByRole("textbox", { name: "Ausschließen" })).toHaveValue(".git, *.tmp ,");
  });

  it("says when both folders are equal", async () => {
    vi.mocked(api.syncCompare).mockResolvedValue([]);
    render(<SyncDialog {...props} onClose={() => {}} />);
    expect(await screen.findByText("Beide Ordner sind gleich.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Synchronisieren" })).toBeDisabled();
  });

  it("shows a failed compare", async () => {
    vi.mocked(api.syncCompare).mockRejectedValue({ type: "fs", kind: "permissionDenied", path: "/srv/x", message: "denied" });
    render(<SyncDialog {...props} onClose={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Vergleich fehlgeschlagen");
  });

  it("cancelling while comparing stops the compare and closes", async () => {
    vi.mocked(api.syncCompare).mockReturnValue(new Promise(() => {}));
    const close = vi.fn();
    render(<SyncDialog {...props} onClose={close} />);
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(api.syncCancel).toHaveBeenCalledWith("s1");
    expect(close).toHaveBeenCalled();
  });

  it("reports deletes that were skipped because the entry changed", async () => {
    vi.mocked(api.syncDelete).mockResolvedValue({ deleted: 0, skipped: 1, failed: 0, first: null });
    render(<SyncDialog {...props} onClose={() => {}} />);
    await screen.findByText("index.html");
    await userEvent.click(screen.getByRole("checkbox", { name: "Auf der Zielseite löschen, was es nur dort gibt" }));
    await userEvent.click(screen.getByRole("button", { name: "Synchronisieren" }));
    await vi.waitFor(() => expect(useNoticeStore.getState().notices).toHaveLength(1));
    expect(useNoticeStore.getState().notices[0]!.message).toContain("seit dem Vergleich geändert");
  });
});
