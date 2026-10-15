import { describe, expect, it } from "vitest";
import type { Entry } from "../../ipc/api";
import { crumbs, formatMode, formatMtime, formatSize, joinPath, parentPath, sortEntries, visibleEntries } from "./entries";

const e = (name: string, kind: Entry["kind"] = { type: "file" }, size = 0, mtime = 0): Entry => ({
  name, kind, size, mtime, mode: 0o100644, owner: "u", nameLossy: false,
});

describe("entries", () => {
  it("sorts folders and folder links first, names naturally and case-insensitively", () => {
    const list = [e("file10"), e("File2"), e("b-dir", { type: "dir" }), e("a-link", { type: "symlink", target: "dir" }), e("z")];
    expect(sortEntries(list, { key: "name", dir: "asc" }).map((x) => x.name)).toEqual(["a-link", "b-dir", "File2", "file10", "z"]);
    expect(sortEntries(list, { key: "name", dir: "desc" }).map((x) => x.name)).toEqual(["b-dir", "a-link", "z", "file10", "File2"]);
  });

  it("sorts by size with folders still first", () => {
    const list = [e("big", { type: "file" }, 10), e("d", { type: "dir" }, 4096), e("small", { type: "file" }, 1)];
    expect(sortEntries(list, { key: "size", dir: "asc" }).map((x) => x.name)).toEqual(["d", "small", "big"]);
  });

  it("hides dot files unless asked", () => {
    const list = [e(".env"), e("a")];
    expect(visibleEntries(list, false, { key: "name", dir: "asc" }).map((x) => x.name)).toEqual(["a"]);
    expect(visibleEntries(list, true, { key: "name", dir: "asc" })).toHaveLength(2);
  });

  it("formats sizes, dates and modes the German way", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1536)).toBe("1,5 KB");
    expect(formatSize(19293798)).toBe("18,4 MB");
    expect(formatMtime(null)).toBe("");
    expect(formatMtime(new Date(2026, 9, 8, 10, 41).getTime() / 1000)).toBe("08.10.2026 10:41");
    expect(formatMode(0o100755)).toBe("rwxr-xr-x");
    expect(formatMode(0o104750)).toBe("rwsr-x---");
    expect(formatMode(null)).toBe("");
  });

  it("builds paths and breadcrumbs", () => {
    expect(parentPath("/var/www")).toBe("/var");
    expect(parentPath("/var")).toBe("/");
    expect(parentPath("/")).toBe("/");
    expect(joinPath("/", "etc")).toBe("/etc");
    expect(joinPath("/var", "www")).toBe("/var/www");
    expect(crumbs("/var/www")).toEqual([
      { label: "/", path: "/" },
      { label: "var", path: "/var" },
      { label: "www", path: "/var/www" },
    ]);
  });
});
