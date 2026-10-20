import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { usePortForwardingStore } from "./portForwarding";
import { useTunnelsStore } from "./tunnels";

vi.mock("../ipc/api");

const rule: api.PortForward = { id: "r-1", kind: "local", bind: "8080", target: "localhost:80", autostart: false };
const store = () => usePortForwardingStore.getState();
const reason: api.CoreError = { type: "internal", message: "weg" };

beforeEach(() => {
  usePortForwardingStore.setState(usePortForwardingStore.getInitialState(), true);
  useTunnelsStore.setState({ states: {} });
  vi.mocked(api.tunnelsClose).mockResolvedValue(undefined);
});

describe("port forwarding store", () => {
  it("adds a connected entry and closes the session it replaces", () => {
    store().connected("h-1", { sessionId: "s-1", forwards: [rule] });
    expect(store().entries["h-1"]).toEqual({ sessionId: "s-1", forwards: [rule], status: { kind: "connected" } });
    store().connected("h-1", { sessionId: "s-2", forwards: [rule] });
    expect(store().entries["h-1"]?.sessionId).toBe("s-2");
    expect(api.tunnelsClose).toHaveBeenCalledWith("s-1");
  });

  it("follows reconnecting, reconnected and lost; ignores another session", () => {
    store().connected("h-1", { sessionId: "s-1", forwards: [rule] });
    store().onEvent("h-1", "s-1", { kind: "reconnecting", attempt: 2, maxAttempts: 3, retryInMs: 4000, reason });
    expect(store().entries["h-1"]?.status).toEqual({ kind: "reconnecting", attempt: 2, maxAttempts: 3 });
    store().onEvent("h-1", "s-1", { kind: "reconnected" });
    expect(store().entries["h-1"]?.status).toEqual({ kind: "connected" });
    store().onEvent("h-1", "s-old", { kind: "lost", error: reason });
    expect(store().entries["h-1"]?.status).toEqual({ kind: "connected" });
    store().onEvent("h-1", "s-1", { kind: "lost", error: reason });
    expect(store().entries["h-1"]?.status).toEqual({ kind: "lost", error: reason });
  });

  it("puts tunnel events under the session id", () => {
    store().connected("h-1", { sessionId: "s-1", forwards: [rule] });
    store().onEvent("h-1", "s-1", { kind: "tunnel", event: { ruleId: "r-1", state: { kind: "active", port: 8080 } } });
    expect(useTunnelsStore.getState().states["s-1"]).toEqual({ "r-1": { kind: "active", port: 8080 } });
  });

  it("disconnect removes the entry and closes its session", async () => {
    store().connected("h-1", { sessionId: "s-1", forwards: [rule] });
    await store().disconnect("h-1");
    expect(store().entries["h-1"]).toBeUndefined();
    expect(api.tunnelsClose).toHaveBeenCalledWith("s-1");
    await store().disconnect("h-unknown");
    expect(api.tunnelsClose).toHaveBeenCalledTimes(1);
  });
});
