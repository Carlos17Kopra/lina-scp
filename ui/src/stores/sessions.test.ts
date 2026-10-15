import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { useNoticeStore } from "./notices";
import { paneIds } from "../screens/terminal/layout";
import { discardEarlySessionEvent, pendingSessionEventCount, sessionEventHandler, sessionEventPatch } from "./sessionEvents";
import { broadcastPeers, focusedPane, type Tab, type TabStatus, useSessionsStore } from "./sessions";
import { getStream, registerStream } from "./streams";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

const sessionEventHandlerFor = (paneId: string) => sessionEventHandler(paneId);

const stream = { channel: { raw: true }, attach: vi.fn(), bind: vi.fn(), ack: vi.fn() };
let sessionHandlers: Array<(event: api.SessionEventDto) => void> = [];

beforeEach(() => {
  let next = 0;
  sessionHandlers = [];
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  vi.mocked(api.newId).mockImplementation(() => `t-${++next}`);
  vi.mocked(api.createOutputStream).mockReturnValue(stream as unknown as api.OutputStream);
  vi.mocked(api.createEventChannel).mockImplementation((handler) => {
    sessionHandlers.push(handler as (event: api.SessionEventDto) => void);
    return { events: true } as never;
  });
  vi.mocked(api.sessionClose).mockResolvedValue(undefined);
});

describe("sessions store — local terminals", () => {
  it("opens numbered local terminal tabs and shows the terminal view", async () => {
    vi.mocked(api.sessionOpenLocal).mockResolvedValueOnce("s-1").mockResolvedValueOnce("s-2");
    await useSessionsStore.getState().openLocal();
    expect(api.sessionOpenLocal).toHaveBeenCalledWith(expect.any(Number), expect.any(Number), stream.channel, { events: true });
    expect(stream.bind).toHaveBeenCalledWith("s-1");
    expect(getStream("t-1")).toBe(stream);
    expect(useUiStore.getState().view).toBe("terminal");
    await useSessionsStore.getState().openLocal();
    expect(useSessionsStore.getState().tabs).toEqual([
      { id: "t-1", title: "Lokales Terminal", kind: "local", sessionId: "s-1", status: "connected", connectedAt: expect.any(Number) },
      { id: "t-2", title: "Lokales Terminal 2", kind: "local", sessionId: "s-2", status: "connected", connectedAt: expect.any(Number) },
    ]);
    expect(useSessionsStore.getState().activeId).toBe("t-2");
    sessionHandlers[0]?.({ kind: "exit", code: 0 });
    expect(useSessionsStore.getState().tabs[0]?.status).toBe("closed");
  });

  it("applies a session exit that arrived before the local terminal opened", async () => {
    let resolve: (id: string) => void = () => {};
    vi.mocked(api.sessionOpenLocal).mockReturnValue(new Promise((r) => (resolve = r)));
    const opening = useSessionsStore.getState().openLocal();
    // e.g. an invalid $SHELL: the child exits before session_open_local returns.
    sessionHandlers[0]?.({ kind: "exit", code: 127 });
    resolve("s-1");
    await opening;
    expect(useSessionsStore.getState().tabs).toMatchObject([{ id: "t-1", sessionId: "s-1", status: "closed" }]);
    expect(pendingSessionEventCount()).toBe(0);
  });

  it("drops a buffered session event when the local terminal fails to open", async () => {
    let reject: (error: unknown) => void = () => {};
    vi.mocked(api.sessionOpenLocal).mockReturnValue(new Promise((_, r) => (reject = r)));
    const opening = useSessionsStore.getState().openLocal();
    sessionHandlers[0]?.({ kind: "exit", code: 1 });
    reject({ type: "io", message: "no pty" });
    await opening;
    expect(useSessionsStore.getState().tabs).toEqual([]);
    expect(pendingSessionEventCount()).toBe(0);
  });

  it("reports a failed start as a notice", async () => {
    vi.mocked(api.sessionOpenLocal).mockRejectedValue({ type: "io", message: "no pty" });
    await useSessionsStore.getState().openLocal();
    expect(useSessionsStore.getState().tabs).toEqual([]);
    expect(useNoticeStore.getState().notices).toMatchObject([{ tone: "red", message: "Lokales Terminal konnte nicht gestartet werden" }]);
  });

  it("closes a session: removes the tab and stream and closes the backend session", async () => {
    registerStream("a", stream as unknown as api.OutputStream);
    useSessionsStore.getState().addTab({ id: "a", title: "web", kind: "ssh", hostId: "h", sessionId: "s-a", status: "connected" });
    await useSessionsStore.getState().closeSession("a");
    expect(useSessionsStore.getState().tabs).toEqual([]);
    expect(getStream("a")).toBeUndefined();
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
  });
});

const pane = (id: string, status: TabStatus = "connected"): Tab => ({
  id,
  title: id,
  kind: "ssh",
  hostId: `h-${id}`,
  sessionId: `s-${id}`,
  status,
});

describe("sessions store — groups and panes", () => {
  it("creates one group per added tab, its id being the pane id", () => {
    const s = useSessionsStore.getState();
    s.addTab(pane("a"));
    s.addTab(pane("b"));
    expect(useSessionsStore.getState().groups).toEqual([
      { id: "a", layout: { type: "pane", paneId: "a" }, focusedPaneId: "a", broadcast: false },
      { id: "b", layout: { type: "pane", paneId: "b" }, focusedPaneId: "b", broadcast: false },
    ]);
    expect(useSessionsStore.getState().activeId).toBe("b");
  });

  it("splits the focused pane into a focused picking pane that remembers its origin, up to 4 panes", () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    const state = useSessionsStore.getState();
    expect(paneIds(state.groups[0]!.layout)).toEqual(["a", "t-1"]);
    expect(state.groups[0]!.focusedPaneId).toBe("t-1");
    expect(focusedPane(state)).toMatchObject({ id: "t-1", status: "picking", origin: { kind: "ssh", hostId: "h-a" } });
    useSessionsStore.getState().split("column");
    useSessionsStore.getState().split("column");
    useSessionsStore.getState().split("row");
    expect(paneIds(useSessionsStore.getState().groups[0]!.layout)).toHaveLength(4);
  });

  it("closes a pane: its session ends, focus moves to the previous pane, the group goes with its last pane", async () => {
    const s = useSessionsStore.getState();
    s.addTab(pane("x"));
    s.addTab(pane("a"));
    s.split("row");
    useSessionsStore.getState().attachSession("t-1", "t-1", { sessionId: "s-new", status: "connected" });
    await useSessionsStore.getState().closeSession("t-1");
    expect(api.sessionClose).toHaveBeenCalledWith("s-new");
    expect(useSessionsStore.getState().groups[1]).toMatchObject({ layout: { type: "pane", paneId: "a" }, focusedPaneId: "a" });
    await useSessionsStore.getState().closeSession("a");
    expect(useSessionsStore.getState()).toMatchObject({ activeId: "x", groups: [{ id: "x" }] });
  });

  it("closes a whole group with all its sessions", async () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().attachSession("t-1", "t-1", { sessionId: "s-2", status: "connected" });
    await useSessionsStore.getState().closeGroup("a");
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
    expect(api.sessionClose).toHaveBeenCalledWith("s-2");
    expect(useSessionsStore.getState()).toMatchObject({ tabs: [], groups: [], activeId: null });
  });

  it("moves focus cyclically and sets ratios within bounds", () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().focusNeighbour(1);
    expect(useSessionsStore.getState().groups[0]!.focusedPaneId).toBe("a");
    useSessionsStore.getState().focusPane("t-1");
    expect(useSessionsStore.getState().groups[0]!.focusedPaneId).toBe("t-1");
    useSessionsStore.getState().setRatio("a", [], 0.99);
    expect(useSessionsStore.getState().groups[0]!.layout).toMatchObject({ ratio: 0.85 });
  });

  it("broadcasts only from the focused pane to connected peers and switches off at one pane", async () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().toggleBroadcast();
    expect(useSessionsStore.getState().groups[0]!.broadcast).toBe(false);
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().attachSession("t-1", "t-1", { sessionId: "s-b", status: "connected" });
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().focusPane("a");
    useSessionsStore.getState().toggleBroadcast();
    expect(broadcastPeers("a")).toEqual(["s-b"]);
    expect(broadcastPeers("t-1")).toEqual([]);
    useSessionsStore.getState().updateTab("t-1", { status: "closed" });
    expect(broadcastPeers("a")).toEqual([]);
    await useSessionsStore.getState().closeSession("t-1");
    await useSessionsStore.getState().closeSession("t-2");
    expect(useSessionsStore.getState().groups[0]!.broadcast).toBe(false);
  });

  it("attaches sessions to picking panes only and applies an event that arrived meanwhile", () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    expect(useSessionsStore.getState().attachSession("a", "a", { sessionId: "s-x" })).toBe(false);
    expect(useSessionsStore.getState().attachSession("zz", "zz", { sessionId: "s-x" })).toBe(false);
    // An exit reported while the pane still shows the picker is buffered, then applied on attach.
    sessionEventHandlerFor("t-1")({ kind: "exit", code: 0 });
    expect(focusedPane(useSessionsStore.getState())?.status).toBe("picking");
    expect(useSessionsStore.getState().attachSession("t-1", "t-1", { sessionId: "s-b", status: "connected" })).toBe(true);
    expect(focusedPane(useSessionsStore.getState())).toMatchObject({ sessionId: "s-b", status: "closed", origin: undefined });
  });

  it("replaces an ended pane with a fresh picking pane (Review Focus 2)", () => {
    useSessionsStore.getState().addTab(pane("a", "closed"));
    const fresh = useSessionsStore.getState().replacePane("a");
    expect(fresh).toBe("t-1");
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
    expect(useSessionsStore.getState().groups[0]).toMatchObject({ id: "a", layout: { paneId: "t-1" }, focusedPaneId: "t-1" });
    // A late event of the old session goes to the old id and cannot touch the new pane.
    sessionEventHandlerFor("a")({ kind: "exit", code: 0 });
    expect(focusedPane(useSessionsStore.getState())).toMatchObject({ id: "t-1", status: "picking", origin: { kind: "ssh", hostId: "h-a" } });
    discardEarlySessionEvent("a");
  });

  it("opens a local shell inside a picking pane", async () => {
    vi.mocked(api.sessionOpenLocal).mockResolvedValue("s-l");
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    await useSessionsStore.getState().openLocal({ paneId: "t-1" });
    expect(useSessionsStore.getState().groups).toHaveLength(1);
    expect(focusedPane(useSessionsStore.getState())).toMatchObject({ id: "t-2", kind: "local", sessionId: "s-l", status: "connected" });
    expect(getStream("t-2")).toBe(stream);
  });

  it("closes the local shell when its picking pane vanished meanwhile", async () => {
    let resolve: (id: string) => void = () => undefined;
    vi.mocked(api.sessionOpenLocal).mockImplementation(() => new Promise((r) => (resolve = r)));
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    const opening = useSessionsStore.getState().openLocal({ paneId: "t-1" });
    useSessionsStore.getState().closeTab("t-1");
    resolve("s-late");
    await opening;
    expect(api.sessionClose).toHaveBeenCalledWith("s-late");
    expect(getStream("t-2")).toBeUndefined();
    expect(useSessionsStore.getState().groups).toHaveLength(1);
  });

  it("keeps the first local shell when the picker fires twice (review C1)", async () => {
    const resolvers: Array<(id: string) => void> = [];
    vi.mocked(api.sessionOpenLocal).mockImplementation(() => new Promise((r) => resolvers.push(r)));
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    const first = useSessionsStore.getState().openLocal({ paneId: "t-1" });
    const second = useSessionsStore.getState().openLocal({ paneId: "t-1" });
    resolvers[0]?.("s-1");
    await first;
    resolvers[1]?.("s-2");
    await second;
    expect(api.sessionClose).toHaveBeenCalledWith("s-2");
    // The surplus shell's exit must not end the live one, and its stream cleanup must not drop the live stream.
    sessionHandlers[1]?.({ kind: "exit", code: 0 });
    const live = focusedPane(useSessionsStore.getState());
    expect(live).toMatchObject({ sessionId: "s-1", status: "connected" });
    expect(getStream(live!.id)).toBe(stream);
  });
});

describe("session event patches for reconnects", () => {
  const reason = { type: "network", kind: "other", message: "connection lost" } as const;

  it("maps reconnecting, reconnected and lost onto the pane", () => {
    vi.spyOn(Date, "now").mockReturnValue(10_000);
    expect(sessionEventPatch({ kind: "reconnecting", attempt: 2, maxAttempts: 8, retryInMs: 2000, reason })).toEqual({
      status: "reconnecting",
      reconnect: { attempt: 2, maxAttempts: 8, retryAt: 12_000, reason },
    });
    const info = { hostKeyAlgorithm: "ssh-ed25519", hostKeySha256: "x", kex: "k", cipher: "c", mac: "m", authMethods: [] };
    expect(sessionEventPatch({ kind: "reconnected", info })).toEqual({
      status: "connected",
      info,
      connectedAt: 10_000,
      reconnect: undefined,
      error: undefined,
    });
    expect(sessionEventPatch({ kind: "lost", reason })).toEqual({ status: "lost", error: reason, reconnect: undefined });
    vi.restoreAllMocks();
  });
});
