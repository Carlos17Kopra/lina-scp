import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel<T> {
    onmessage: (response: T) => void;
    constructor(onmessage?: (response: T) => void) {
      this.onmessage = onmessage ?? (() => undefined);
    }
  }
  return { invoke, Channel };
});

import {
  createOutputStream,
  hostDelete,
  hostsList,
  importApply,
  sessionReconnectNow,
  sessionReconnectStop,
  sessionWrite,
  type Host,
} from "./api";

const host = { id: "h1", name: "web-01" } as unknown as Host;

describe("command wrappers", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("resolve with the command data", async () => {
    invoke.mockResolvedValue([host]);
    await expect(hostsList()).resolves.toEqual([host]);
    expect(invoke).toHaveBeenCalledWith("hosts_list");
  });

  it("reject with the CoreError", async () => {
    const error = { type: "network", kind: "timeout", message: "timed out" };
    invoke.mockRejectedValue(error);
    await expect(hostsList()).rejects.toEqual(error);
  });

  it("map unit results to undefined", async () => {
    invoke.mockResolvedValue(null);
    await expect(hostDelete("h1")).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("host_delete", { id: "h1" });
  });

  it("encode terminal input as UTF-8 bytes", async () => {
    invoke.mockResolvedValue(null);
    await sessionWrite("s1", "hé");
    expect(invoke).toHaveBeenCalledWith("session_write", { sessionId: "s1", data: [104, 195, 169] });
  });

  it("keep terminal input in order: one write in flight per session, the rest batched", async () => {
    const pending: Array<(value: null) => void> = [];
    invoke.mockImplementation((command: string) =>
      command === "session_write" ? new Promise((resolve) => pending.push(resolve)) : Promise.resolve(null),
    );
    const release = (index: number) => {
      const resolve = pending[index];
      if (!resolve) throw new Error(`write ${index} was never sent`);
      resolve(null);
    };

    const first = sessionWrite("s1", "a");
    const second = sessionWrite("s1", "b");
    const third = sessionWrite("s1", new Uint8Array([0x80]));
    const other = sessionWrite("s2", "x");
    expect(invoke.mock.calls).toEqual([
      ["session_write", { sessionId: "s1", data: [97] }],
      ["session_write", { sessionId: "s2", data: [120] }],
    ]);

    release(0);
    await first;
    expect(invoke).toHaveBeenLastCalledWith("session_write", { sessionId: "s1", data: [98, 0x80] });
    release(1);
    release(2);
    await Promise.all([second, third, other]);

    void sessionWrite("s1", "c");
    expect(invoke).toHaveBeenLastCalledWith("session_write", { sessionId: "s1", data: [99] });
    release(3);
  });

  it("reject a batched write when its own send fails, and keep going", async () => {
    let call = 0;
    invoke.mockImplementation(() => (++call === 2 ? Promise.reject({ type: "io", message: "x" }) : Promise.resolve(null)));
    const first = sessionWrite("s3", "a");
    const second = sessionWrite("s3", "b");
    await expect(first).resolves.toBeUndefined();
    await expect(second).rejects.toEqual({ type: "io", message: "x" });
    await expect(sessionWrite("s3", "c")).resolves.toBeUndefined();
  });

  it("pass import resolutions keyed by id", async () => {
    invoke.mockResolvedValue({ imported: 1, skipped: 0, secretsRestored: 0 });
    await importApply("/tmp/x.linascp", null, { h1: "replace" });
    expect(invoke).toHaveBeenCalledWith("import_apply", {
      path: "/tmp/x.linascp",
      passphrase: null,
      resolutions: { h1: "replace" },
    });
  });

  it("wrap the reconnect commands", async () => {
    invoke.mockResolvedValue(null);
    await sessionReconnectNow("s1");
    expect(invoke).toHaveBeenLastCalledWith("session_reconnect_now", { sessionId: "s1" });
    await sessionReconnectStop("s1");
    expect(invoke).toHaveBeenLastCalledWith("session_reconnect_stop", { sessionId: "s1" });
  });
});

describe("createOutputStream", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(null);
  });

  it("buffers output until a consumer attaches", () => {
    const stream = createOutputStream();
    stream.channel.onmessage(new Uint8Array([1, 2]).buffer);
    const received: number[][] = [];
    stream.attach((bytes) => received.push(Array.from(bytes)));
    stream.channel.onmessage(new Uint8Array([3]).buffer);
    expect(received).toEqual([[1, 2], [3]]);
  });

  it("buffers again after the consumer detached", () => {
    const stream = createOutputStream();
    const received: number[][] = [];
    const detach = stream.attach((bytes) => received.push(Array.from(bytes)));
    detach();
    stream.channel.onmessage(new Uint8Array([9]).buffer);
    expect(received).toEqual([]);
    stream.attach((bytes) => received.push(Array.from(bytes)));
    expect(received).toEqual([[9]]);
  });

  it("accumulates acks until the session id is known", () => {
    const stream = createOutputStream();
    stream.ack(100);
    stream.ack(28);
    expect(invoke).not.toHaveBeenCalled();
    stream.bind("s1");
    expect(invoke).toHaveBeenCalledWith("session_ack", { sessionId: "s1", bytes: 128 });
    stream.ack(5);
    expect(invoke).toHaveBeenLastCalledWith("session_ack", { sessionId: "s1", bytes: 5 });
  });

  it("re-queues bytes of a rejected ack and sends the total with the next ack", async () => {
    const stream = createOutputStream();
    stream.bind("s1");
    invoke.mockRejectedValueOnce({ type: "internal", message: "boom" });
    stream.ack(10);
    await new Promise((resolve) => setTimeout(resolve, 0));
    invoke.mockResolvedValue(null);
    stream.ack(5);
    expect(invoke).toHaveBeenLastCalledWith("session_ack", { sessionId: "s1", bytes: 15 });
  });
});
