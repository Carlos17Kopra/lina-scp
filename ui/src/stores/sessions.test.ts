import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { useNoticeStore } from "./notices";
import { pendingSessionEventCount } from "./sessionEvents";
import { useSessionsStore } from "./sessions";
import { getStream, registerStream } from "./streams";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

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
      { id: "t-1", title: "Lokales Terminal", kind: "local", sessionId: "s-1", status: "connected" },
      { id: "t-2", title: "Lokales Terminal 2", kind: "local", sessionId: "s-2", status: "connected" },
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
