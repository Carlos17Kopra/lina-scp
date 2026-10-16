import { describe, expect, it } from "vitest";
import type { Entry } from "../../ipc/api";
import type { SftpTab } from "../../stores/sftp";
import { dropDirAt, requestFor } from "./transferTargets";

const entry = (name: string, type: "file" | "dir" = "file", nameLossy = false): Entry =>
  ({ name, kind: { type }, size: 1, mtime: 0, mode: 0, owner: null, nameLossy }) as Entry;

function tab(status: SftpTab["status"] = "connected"): SftpTab {
  const pane = (path: string, entries: Entry[]) => ({ path, entries, loading: false, selection: [], cursor: null, anchor: null,
    sort: { key: "name", dir: "asc" }, showHidden: false, editingPath: false, token: 0 });
  return {
    id: "t", hostId: "h", sessionId: "s", title: "web", status, info: {} as never, version: 3, active: "local",
    panes: { local: pane("/home/me", [entry("a.txt"), entry("docs", "dir"), entry("bad", "file", true)]), remote: pane("/srv", [entry("www", "dir")]) },
  } as SftpTab;
}

describe("requestFor", () => {
  it("uploads from the local pane into the remote directory", () => {
    expect(requestFor(tab(), "local", ["a.txt", "docs"])).toEqual({
      direction: "upload", sources: ["/home/me/a.txt", "/home/me/docs"], targetDir: "/srv",
    });
  });

  it("downloads into a drop directory", () => {
    expect(requestFor(tab(), "remote", ["www"], "/home/me/docs")).toEqual({
      direction: "download", sources: ["/srv/www"], targetDir: "/home/me/docs",
    });
  });

  it("skips .. and lossy names and returns null when nothing is left", () => {
    expect(requestFor(tab(), "local", ["..", "bad"])).toBeNull();
    expect(requestFor(tab(), "local", ["..", "a.txt"])?.sources).toEqual(["/home/me/a.txt"]);
  });

  it("needs a connected remote", () => {
    expect(requestFor(tab("reconnecting"), "local", ["a.txt"])).toBeNull();
    expect(requestFor(tab("lost"), "remote", ["www"])).toBeNull();
  });
});

describe("dropDirAt", () => {
  it("uses a directory row, else the pane path", () => {
    expect(dropDirAt(tab(), "local", "docs")).toBe("/home/me/docs");
    expect(dropDirAt(tab(), "local", "a.txt")).toBe("/home/me");
    expect(dropDirAt(tab(), "local", null)).toBe("/home/me");
    expect(dropDirAt(tab(), "local", "..")).toBe("/home/me");
  });
});
