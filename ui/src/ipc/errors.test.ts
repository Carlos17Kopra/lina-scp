import { describe, expect, it } from "vitest";
import { errorDetails, errorMessage, isCoreError } from "./errors";

describe("isCoreError", () => {
  it("accepts every backend error category", () => {
    expect(isCoreError({ type: "network", kind: "timeout", message: "timed out" })).toBe(true);
    expect(isCoreError({ type: "auth", rejectedMethods: ["password"], message: "denied" })).toBe(true);
    expect(isCoreError({ type: "cancelled" })).toBe(true);
  });

  it("rejects anything else", () => {
    expect(isCoreError({ type: "weird" })).toBe(false);
    expect(isCoreError(new Error("x"))).toBe(false);
    expect(isCoreError(null)).toBe(false);
    expect(isCoreError("network")).toBe(false);
  });
});

describe("errorMessage / errorDetails", () => {
  it("uses the backend message or the category", () => {
    expect(errorMessage({ type: "io", message: "disk full" })).toBe("disk full");
    expect(errorMessage({ type: "cancelled" })).toBe("cancelled");
    expect(errorMessage(new Error("boom"))).toBe("boom");
    expect(errorMessage(42)).toBe("42");
  });

  it("renders copyable details", () => {
    expect(errorDetails({ type: "config", message: "bad" })).toBe('{\n  "type": "config",\n  "message": "bad"\n}');
    expect(errorDetails("plain")).toBe("plain");
  });
});
