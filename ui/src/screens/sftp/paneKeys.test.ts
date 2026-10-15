import { describe, expect, it } from "vitest";
import { moveCursor, rangeBetween, typeAhead } from "./paneKeys";

const names = ["..", "a", "b", "c", "d"];

describe("paneKeys", () => {
  it("moves the cursor with arrows, Home/End and pages", () => {
    expect(moveCursor(names, null, "ArrowDown", 2)).toBe("..");
    expect(moveCursor(names, "a", "ArrowDown", 2)).toBe("b");
    expect(moveCursor(names, "a", "ArrowUp", 2)).toBe("..");
    expect(moveCursor(names, "..", "ArrowUp", 2)).toBe("..");
    expect(moveCursor(names, "b", "End", 2)).toBe("d");
    expect(moveCursor(names, "b", "Home", 2)).toBe("..");
    expect(moveCursor(names, "a", "PageDown", 2)).toBe("c");
    expect(moveCursor(names, "d", "PageDown", 2)).toBe("d");
  });

  it("selects ranges in both directions without ..", () => {
    expect(rangeBetween(names, "d", "b")).toEqual(["b", "c", "d"]);
    expect(rangeBetween(names, "..", "b")).toEqual(["a", "b"]);
  });

  it("jumps to the first name with the typed prefix after the cursor, wrapping", () => {
    expect(typeAhead(["..", "apple", "banana", "berry"], "b", null)).toBe("banana");
    expect(typeAhead(["..", "apple", "banana", "berry"], "be", "banana")).toBe("berry");
    expect(typeAhead(["..", "apple", "Banana"], "b", "apple")).toBe("Banana");
    expect(typeAhead(["..", "apple"], "z", "apple")).toBeNull();
  });
});
