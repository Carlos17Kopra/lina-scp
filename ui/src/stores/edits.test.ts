import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import type { EditFile } from "../ipc/api";
import { conflictOf, dirtyCount, useEditsStore } from "./edits";
import { useNoticeStore } from "./notices";
import { useSftpStore } from "./sftp";
import { useTransfersStore } from "./transfers";

vi.mock("../ipc/api");

export const editFile = (patch: Partial<EditFile> = {}): EditFile => ({
  id: 1, name: "a.conf", remotePath: "/etc/a.conf", state: "synced", dirty: false, error: null, ...patch,
});
const event = (patch: Partial<EditFile>) => useEditsStore.getState().onEvent("t", { kind: "file", file: editFile(patch) });

beforeEach(() => {
  useEditsStore.setState(useEditsStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
});

describe("edits store", () => {
  it("upserts files by id in arrival order", () => {
    event({ id: 1, state: "downloading" });
    event({ id: 2, name: "b" });
    event({ id: 1, state: "synced" });
    expect(useEditsStore.getState().byTab.t!.map((f) => [f.id, f.state])).toEqual([[1, "synced"], [2, "synced"]]);
  });

  it("counts dirty files and finds the first conflict", () => {
    event({ id: 1, dirty: true, state: "changed" });
    event({ id: 2, dirty: true, state: "conflict" });
    event({ id: 3 });
    const files = useEditsStore.getState().byTab.t;
    expect(dirtyCount(files)).toBe(2);
    expect(conflictOf(files)?.id).toBe(2);
    expect(dirtyCount(undefined)).toBe(0);
  });

  it("opens remote files through the session and local files in place", async () => {
    vi.mocked(api.editOpen).mockResolvedValue(1);
    vi.mocked(api.localEdit).mockResolvedValue();
    await useEditsStore.getState().open("t", "remote", "/srv/a.txt");
    expect(api.editOpen).toHaveBeenCalledWith("s1", "/srv/a.txt");
    expect(useTransfersStore.getState().byTab.t?.view).toBe("edits");
    await useEditsStore.getState().open("t", "local", "/l/a.txt");
    expect(api.localEdit).toHaveBeenCalledWith("/l/a.txt");
  });

  it("explains a missing editor and other failures as notices", async () => {
    vi.mocked(api.editOpen).mockRejectedValueOnce({ type: "config", message: "no text editor found" });
    await useEditsStore.getState().open("t", "remote", "/srv/a.txt");
    vi.mocked(api.localEdit).mockRejectedValueOnce({ type: "fs", kind: "notFound", path: "/l/x", message: "gone" });
    await useEditsStore.getState().open("t", "local", "/l/x");
    expect(useNoticeStore.getState().notices.map((n) => n.message)).toEqual([
      "Kein Editor gefunden. In den Einstellungen festlegen.",
      "Datei konnte nicht geöffnet werden",
    ]);
  });

  it("stop names a kept copy", async () => {
    event({ id: 1, dirty: true, state: "changed" });
    vi.mocked(api.editStop).mockResolvedValue("/c/x/a.conf");
    await useEditsStore.getState().stop("t", 1);
    expect(api.editStop).toHaveBeenCalledWith("s1", 1);
    expect(useEditsStore.getState().byTab.t).toEqual([]);
    expect(useNoticeStore.getState().notices[0]?.message).toBe("Nicht hochgeladene Änderungen liegen unter /c/x/a.conf");
  });

  it("resolve answers the conflict and leaves the conflict state at once", async () => {
    event({ id: 1, dirty: true, state: "conflict" });
    vi.mocked(api.editResolve).mockResolvedValue();
    await useEditsStore.getState().resolve("t", 1, true);
    expect(api.editResolve).toHaveBeenCalledWith("s1", 1, true);
    expect(conflictOf(useEditsStore.getState().byTab.t)).toBeUndefined();
  });
});
