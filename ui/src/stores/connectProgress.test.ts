import { describe, expect, it } from "vitest";
import type { ConnectEvent } from "../ipc/api";
import { applyConnectEvent, asCoreError, EMPTY_PROGRESS, MAX_LOG_LINES, progressRatio, type StepProgress } from "./connectProgress";

const run = (events: ConnectEvent[]): StepProgress => events.reduce(applyConnectEvent, EMPTY_PROGRESS);

describe("applyConnectEvent", () => {
  it("tracks the planned, current, done and failed steps", () => {
    const progress = run([
      { kind: "steps", steps: ["tcp", "hostKey", "auth"] },
      { kind: "stepStarted", step: "tcp" },
      { kind: "stepDone", step: "tcp" },
      { kind: "stepStarted", step: "hostKey" },
    ]);
    expect(progress).toMatchObject({ steps: ["tcp", "hostKey", "auth"], done: ["tcp"], current: "hostKey" });
    expect(progressRatio(progress)).toBeCloseTo(1 / 3);
    const failed = applyConnectEvent(progress, {
      kind: "stepFailed",
      step: "hostKey",
      error: { type: "hostKeyRejected", message: "rejected" },
    });
    expect(failed).toMatchObject({ current: undefined, failedStep: "hostKey" });
  });

  it("keeps extra fields and ignores prompt/connected/failed events", () => {
    const state = { ...EMPTY_PROGRESS, attempt: 2 };
    const next = applyConnectEvent(state, { kind: "connected", sessionId: "s1" });
    expect(next).toBe(state);
    expect(applyConnectEvent(state, { kind: "log", line: "debug1: x" })).toMatchObject({ attempt: 2, log: ["debug1: x"] });
  });

  it("caps the log", () => {
    let state: StepProgress = EMPTY_PROGRESS;
    for (let i = 0; i < MAX_LOG_LINES + 5; i += 1) state = applyConnectEvent(state, { kind: "log", line: `l${i}` });
    expect(state.log).toHaveLength(MAX_LOG_LINES);
    expect(state.log[0]).toBe("l5");
  });

  it("starts over when a new step plan arrives", () => {
    const progress = run([
      { kind: "steps", steps: ["tcp"] },
      { kind: "stepDone", step: "tcp" },
      { kind: "steps", steps: ["tcp", "auth"] },
    ]);
    expect(progress).toMatchObject({ steps: ["tcp", "auth"], done: [] });
    expect(progressRatio(EMPTY_PROGRESS)).toBe(0);
  });
});

describe("asCoreError", () => {
  it("passes CoreErrors through and wraps anything else", () => {
    const error = { type: "network", kind: "dns", message: "nope" } as const;
    expect(asCoreError(error)).toBe(error);
    expect(asCoreError(new Error("boom"))).toEqual({ type: "internal", message: "boom" });
  });
});
