import { describe, expect, it } from "vitest";
import { formatSince } from "./format";

describe("formatSince", () => {
  it("shows the local clock time and the elapsed time", () => {
    const since = new Date(2026, 9, 15, 14, 2).getTime();
    expect(formatSince(since, since + 30_000)).toBe("14:02 (0 Min.)");
    expect(formatSince(since, since + (2 * 60 + 14) * 60_000)).toBe("14:02 (2 Std. 14 Min.)");
    expect(formatSince(since, since - 5_000)).toBe("14:02 (0 Min.)");
  });
});
