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

import { createOutputStream, hostDelete, hostsList, importApply, sessionWrite, type Host } from "./api";

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

  it("pass import resolutions keyed by id", async () => {
    invoke.mockResolvedValue({ imported: 1, skipped: 0, secretsRestored: 0 });
    await importApply("/tmp/x.linascp", null, { h1: "replace" });
    expect(invoke).toHaveBeenCalledWith("import_apply", {
      path: "/tmp/x.linascp",
      passphrase: null,
      resolutions: { h1: "replace" },
    });
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
});
