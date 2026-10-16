import { describe, expect, it } from "vitest";
import type { DiffStatus, Difference } from "../../ipc/api";
import { planSync, requestsFor } from "./syncPlan";

const file = (size: number, mtime: number) => ({ dir: false, size, mtime });
const diff = (status: DiffStatus, rel: string = status): Difference => ({
  rel,
  status,
  local: status === "onlyRemote" ? null : file(1, 100),
  remote: status === "onlyLocal" ? null : file(2, 200),
});
const ALL: DiffStatus[] = ["onlyLocal", "onlyRemote", "localNewer", "remoteNewer", "sizeDiffers", "typeMismatch"];
const actions = (direction: "upload" | "download" | "both", deleteExtra = false) =>
  planSync(ALL.map((s) => diff(s)), direction, deleteExtra).map((r) => r.action ?? `skip:${r.reason}`);

describe("planSync", () => {
  it("uploads", () => {
    expect(actions("upload")).toEqual(["upload", "skip:onlyRemote", "upload", "skip:remoteNewer", "upload", "skip:typeMismatch"]);
    expect(actions("upload", true)[1]).toBe("deleteRemote");
  });

  it("downloads", () => {
    expect(actions("download")).toEqual(["skip:onlyLocal", "download", "skip:localNewer", "download", "download", "skip:typeMismatch"]);
    expect(actions("download", true)[0]).toBe("deleteLocal");
  });

  it("syncs both ways without deleting", () => {
    expect(actions("both", true)).toEqual(["upload", "download", "upload", "download", "skip:sizeDiffers", "skip:typeMismatch"]);
  });
});

describe("requestsFor", () => {
  it("expects the target as the compare saw it, and nothing for directories or missing targets", () => {
    const rows = planSync(
      [
        diff("localNewer", "a"),
        diff("onlyLocal", "new"),
        { rel: "gone", status: "onlyRemote", local: null, remote: { dir: true, size: 0, mtime: 5 } },
        diff("onlyRemote", "extra"),
      ],
      "upload",
      true,
    );
    expect(requestsFor(rows)).toEqual({
      upload: [{ rel: "a", expected: { size: 2, mtime: 200 } }, { rel: "new", expected: null }],
      download: [],
      deleteLocal: [],
      deleteRemote: [{ rel: "gone", expected: null }, { rel: "extra", expected: { size: 2, mtime: 200 } }],
    });
  });
});
