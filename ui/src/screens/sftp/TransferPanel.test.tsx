import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Job } from "../../ipc/api";
import { useEditsStore } from "../../stores/edits";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { TransferPanel } from "./TransferPanel";

vi.mock("../../ipc/api");

const job = (patch: Partial<Job>): Job => ({
  id: 1, batchId: 1, direction: "upload", source: "/l/a", target: "/srv/a", rel: "a.zip", kind: "file",
  size: 1000, done: 0, state: "queued", note: null, error: null, ...patch,
});
const event = (j: Partial<Job>) => useTransfersStore.getState().onEvent("t", { kind: "job", job: job(j) });

beforeEach(() => {
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useEditsStore.setState(useEditsStore.getInitialState(), true);
  useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
});

describe("TransferPanel", () => {
  it("is hidden without jobs", () => {
    const { container } = render(<TransferPanel tabId="t" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("sorts jobs into the three views with counters", async () => {
    event({ id: 1, state: "running", done: 640, rel: "theme.zip" });
    event({ id: 2, state: "done", rel: "logo.svg", direction: "download", target: "/home/me/logo.svg" });
    event({ id: 3, state: "failed", rel: ".htaccess", error: { type: "fs", kind: "permissionDenied", path: "/srv/.htaccess", message: "denied" } });
    render(<TransferPanel tabId="t" />);
    const tabs = screen.getByRole("tablist", { name: "Übertragungen" });
    expect(within(tabs).getByRole("tab", { name: "Warteschlange 1" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("theme.zip")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "64");
    await userEvent.click(within(tabs).getByRole("tab", { name: "Fehler 1" }));
    expect(screen.getByText("Keine Berechtigung")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: ".htaccess wiederholen" }));
    expect(api.transferRetry).toHaveBeenCalledWith("s1", 3);
    await userEvent.click(within(tabs).getByRole("tab", { name: "Abgeschlossen 1" }));
    await userEvent.click(screen.getByRole("button", { name: "Leeren" }));
    expect(api.transferClear).toHaveBeenCalledWith("s1");
  });

  it("cancels a queued job and collapses to a summary", async () => {
    event({ id: 1, state: "running", rel: "a.zip" });
    event({ id: 2, state: "done", rel: "b" });
    render(<TransferPanel tabId="t" />);
    await userEvent.click(screen.getByRole("button", { name: "a.zip abbrechen" }));
    expect(api.transferCancel).toHaveBeenCalledWith("s1", { type: "job", id: 1 });
    await userEvent.click(screen.getByRole("button", { name: "Übertragungen einklappen" }));
    expect(screen.getByText("1 laufen · 1 fertig · 0 Fehler")).toBeInTheDocument();
    expect(screen.queryByText("a.zip")).not.toBeInTheDocument();
  });

  it("shows skip notes and network errors in German, never the raw message", () => {
    event({ id: 1, state: "skipped", rel: "link", note: "dirLink" });
    event({ id: 2, state: "failed", rel: "x", error: { type: "network", kind: "other", message: "connection lost" } });
    useTransfersStore.getState().setView("t", "done");
    const { rerender } = render(<TransferPanel tabId="t" />);
    expect(screen.getByText("Link auf Ordner – übersprungen")).toBeInTheDocument();
    useTransfersStore.getState().setView("t", "failed");
    rerender(<TransferPanel tabId="t" />);
    expect(screen.getByText("Verbindung unterbrochen")).toBeInTheDocument();
    expect(screen.queryByText("connection lost")).not.toBeInTheDocument();
  });

  it("lists edited files with their state and actions, also without jobs", async () => {
    vi.mocked(api.editReopen).mockResolvedValue();
    vi.mocked(api.editStop).mockResolvedValue(null);
    const file = (patch: Partial<api.EditFile>) => useEditsStore.getState().onEvent("t", { kind: "file", file: {
      id: 1, name: "a.conf", remotePath: "/etc/nginx/a.conf", state: "synced", dirty: false, error: null, ...patch,
    } });
    file({});
    file({ id: 2, name: "b.conf", state: "changed", dirty: true });
    file({ id: 3, name: "c.conf", state: "uploading" });
    file({ id: 4, name: "d.conf", state: "conflict" });
    file({ id: 5, name: "e.conf", state: "failed", error: { type: "fs", kind: "permissionDenied", path: "/etc/e.conf", message: "denied" } });
    render(<TransferPanel tabId="t" />);
    await userEvent.click(screen.getByRole("tab", { name: "Bearbeitet 5" }));
    for (const text of ["Synchron", "Geändert", "Wird hochgeladen", "Auf dem Server geändert", "Keine Berechtigung"]) {
      expect(screen.getByText(text)).toBeInTheDocument();
    }
    expect(screen.getAllByText("/etc/nginx")).toHaveLength(5);
    await userEvent.click(screen.getByRole("button", { name: "a.conf im Editor öffnen" }));
    expect(api.editReopen).toHaveBeenCalledWith("s1", 1);
    await userEvent.click(screen.getByRole("button", { name: "Bearbeitung von a.conf beenden" }));
    expect(api.editStop).toHaveBeenCalledWith("s1", 1);
    await vi.waitFor(() => expect(screen.queryByText("Synchron")).not.toBeInTheDocument());
  });
});
