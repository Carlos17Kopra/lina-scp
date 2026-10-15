import { describe, expect, it } from "vitest";
import {
  arrange,
  clampRatio,
  type Layout,
  leaf,
  neighbour,
  paneIds,
  removePane,
  renamePane,
  setRatio,
  splitPane,
} from "./layout";

// a | (b / c)
const three: Layout = {
  type: "split",
  dir: "row",
  ratio: 0.5,
  a: leaf("a"),
  b: { type: "split", dir: "column", ratio: 0.5, a: leaf("b"), b: leaf("c") },
};

describe("layout", () => {
  it("splits a pane: the new one goes right / below at 50 %", () => {
    expect(splitPane(leaf("a"), "a", "row", "b")).toEqual({ type: "split", dir: "row", ratio: 0.5, a: leaf("a"), b: leaf("b") });
    expect(paneIds(splitPane(three, "b", "row", "d"))).toEqual(["a", "b", "d", "c"]);
    expect(splitPane(three, "zz", "row", "d")).toEqual(three);
  });

  it("removes a pane: the sibling subtree takes the split's place; the last pane yields null", () => {
    expect(removePane(three, "a")).toEqual(three.type === "split" ? three.b : null);
    expect(removePane(three, "c")).toEqual({ type: "split", dir: "row", ratio: 0.5, a: leaf("a"), b: leaf("b") });
    expect(removePane(leaf("a"), "a")).toBeNull();
    expect(removePane(three, "zz")).toBe(three);
  });

  it("renames a pane in place", () => {
    expect(paneIds(renamePane(three, "b", "n"))).toEqual(["a", "n", "c"]);
  });

  it("walks panes cyclically in tree order", () => {
    expect(paneIds(three)).toEqual(["a", "b", "c"]);
    expect(neighbour(three, "c", 1)).toBe("a");
    expect(neighbour(three, "a", -1)).toBe("c");
    expect(neighbour(three, "b", 1)).toBe("c");
    expect(neighbour(three, "zz", 1)).toBe("a");
  });

  it("clamps ratios, also for nested paths and garbage input", () => {
    expect(clampRatio(0.01)).toBe(0.15);
    expect(clampRatio(2)).toBe(0.85);
    expect(clampRatio(Number.NaN)).toBe(0.5);
    const moved = setRatio(three, ["b"], 0.7);
    expect(moved.type === "split" && moved.b.type === "split" && moved.b.ratio).toBe(0.7);
    expect(setRatio(three, [], 0.99).type === "split" && (setRatio(three, [], 0.99) as { ratio: number }).ratio).toBe(0.85);
  });

  it("arranges panes and dividers as fractions of the area", () => {
    const { panes, dividers } = arrange(three);
    expect(panes).toEqual([
      { paneId: "a", rect: { x: 0, y: 0, w: 0.5, h: 1 } },
      { paneId: "b", rect: { x: 0.5, y: 0, w: 0.5, h: 0.5 } },
      { paneId: "c", rect: { x: 0.5, y: 0.5, w: 0.5, h: 0.5 } },
    ]);
    expect(dividers).toEqual([
      { path: [], dir: "row", ratio: 0.5, area: { x: 0, y: 0, w: 1, h: 1 } },
      { path: ["b"], dir: "column", ratio: 0.5, area: { x: 0.5, y: 0, w: 0.5, h: 1 } },
    ]);
  });
});
