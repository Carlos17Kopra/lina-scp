import { describe, expect, it } from "vitest";
import { nextIndex } from "./roving";

describe("nextIndex", () => {
  it("moves forward and backward horizontally with wrap-around", () => {
    expect(nextIndex("ArrowRight", 0, 3, "horizontal")).toBe(1);
    expect(nextIndex("ArrowRight", 2, 3, "horizontal")).toBe(0);
    expect(nextIndex("ArrowLeft", 0, 3, "horizontal")).toBe(2);
  });

  it("ignores keys of the other orientation", () => {
    expect(nextIndex("ArrowDown", 0, 3, "horizontal")).toBeNull();
    expect(nextIndex("ArrowRight", 0, 3, "vertical")).toBeNull();
    expect(nextIndex("ArrowDown", 0, 3, "vertical")).toBe(1);
    expect(nextIndex("Enter", 0, 3, "both")).toBeNull();
  });

  it("accepts both axes for 'both'", () => {
    expect(nextIndex("ArrowDown", 0, 3, "both")).toBe(1);
    expect(nextIndex("ArrowLeft", 1, 3, "both")).toBe(0);
  });

  it("skips disabled items and supports Home/End", () => {
    const disabled = (i: number) => i === 1;
    expect(nextIndex("ArrowRight", 0, 3, "horizontal", disabled)).toBe(2);
    expect(nextIndex("Home", 2, 3, "horizontal", (i) => i === 0)).toBe(1);
    expect(nextIndex("End", 0, 3, "horizontal", (i) => i === 2)).toBe(1);
  });
});
