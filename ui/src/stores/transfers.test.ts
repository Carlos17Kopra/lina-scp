import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import type { Job } from "../ipc/api";
import { useSftpStore } from "./sftp";
import { openCount, speedOf, useTransfersStore } from "./transfers";

vi.mock("../ipc/api");

const job = (patch: Partial<Job>): Job => ({
  id: 1, batchId: 1, direction: "upload", source: "/l/a", target: "/r/a", rel: "a", kind: "file",
  size: 100, done: 0, state: "queued", note: null, error: null, ...patch,
});
const store = useTransfersStore.getState;

beforeEach(() => {
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useSftpStore.setState(useSftpStore.getInitialState(), true);
});

describe("transfers store", () => {
  it("tracks jobs in order and sorts them into the three views", () => {
    store().onEvent("t", { kind: "job", job: job({ id: 1 }) });
    store().onEvent("t", { kind: "job", job: job({ id: 2, state: "failed" }) });
    store().onEvent("t", { kind: "job", job: job({ id: 1, state: "done", done: 100 }) });
    const t = store().byTab.t!;
    expect(t.order).toEqual([1, 2]);
    expect(t.jobs[1]!.state).toBe("done");
    expect(openCount(t)).toBe(0);
    store().onEvent("t", { kind: "job", job: job({ id: 3, state: "paused" }) });
    expect(openCount(store().byTab.t)).toBe(1);
  });

  it("applies progress and computes the speed over the last two seconds", () => {
    store().onEvent("t", { kind: "job", job: job({ id: 1, state: "running" }) });
    store().onEvent("t", { kind: "progress", jobId: 1, done: 0 }, 1000);
    store().onEvent("t", { kind: "progress", jobId: 1, done: 4000 }, 2000);
    store().onEvent("t", { kind: "progress", jobId: 1, done: 8000 }, 3000);
    const t = store().byTab.t!;
    expect(t.jobs[1]!.done).toBe(8000);
    expect(speedOf(t.samples[1]!, 3000)).toBe(4000);
    expect(speedOf([], 3000)).toBeNull();
  });

  it("queues conflicts and resolves the first one", async () => {
    useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
    store().onEvent("t", { kind: "conflict", jobId: 5, source: { size: 2, mtime: 2 }, target: { size: 1, mtime: 1 } });
    store().onEvent("t", { kind: "conflict", jobId: 6, source: { size: 2, mtime: 2 }, target: { size: 1, mtime: 1 } });
    await store().resolve("t", "overwrite", true);
    expect(api.transferResolve).toHaveBeenCalledWith("s1", 5, "overwrite", true);
    expect(store().byTab.t!.conflicts.map((c) => c.jobId)).toEqual([6]);
  });

  it("drops conflicts of jobs that ended otherwise", () => {
    store().onEvent("t", { kind: "conflict", jobId: 5, source: { size: 2, mtime: 2 }, target: { size: 1, mtime: 1 } });
    store().onEvent("t", { kind: "job", job: job({ id: 5, state: "cancelled" }) });
    expect(store().byTab.t!.conflicts).toEqual([]);
  });

  it("clear removes finished jobs locally and in the backend", async () => {
    useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
    store().onEvent("t", { kind: "job", job: job({ id: 1, state: "done" }) });
    store().onEvent("t", { kind: "job", job: job({ id: 2, state: "failed" }) });
    await store().clear("t");
    expect(api.transferClear).toHaveBeenCalledWith("s1");
    expect(store().byTab.t!.order).toEqual([2]);
  });

  it("enqueue passes the session id and opens the queue view", async () => {
    useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
    vi.mocked(api.transferEnqueue).mockResolvedValue(3);
    store().setView("t", "done");
    await store().enqueue("t", "download", ["/r/a"], "/l");
    expect(api.transferEnqueue).toHaveBeenCalledWith("s1", "download", ["/r/a"], "/l");
    expect(store().byTab.t!.view).toBe("queue");
  });
});
