import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { makeHost, makeProfile, makeSessionInfo, withUsage } from "../test/fixtures";
import { MAX_ATTEMPTS, nextRetryDelay, useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useNoticeStore } from "./notices";
import { pendingSessionEventCount, resetSessionEvents } from "./sessionEvents";
import { useSessionsStore } from "./sessions";
import { useSftpStore } from "./sftp";
import { getStream } from "./streams";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

const deploy = makeProfile();
const web = makeHost({ id: "h-web", name: "web-prod-01" });
const viaProfile = makeHost({ id: "h-prof", name: "kunde-shop", auth: { kind: "profile", profileId: deploy.id } });

type Deferred = { resolve(id: string): void; reject(error: unknown): void };
let attempts: Deferred[] = [];
let channels: Array<(event: never) => void> = [];
const stream = { channel: { stream: true }, attach: vi.fn(), bind: vi.fn(), ack: vi.fn() };

/** Event handler of attempt `n` (each attempt creates the events channel first, then the session channel). */
const eventsOf = (n: number) => channels[n * 2] as (event: api.ConnectEvent) => void;
const sessionEventsOf = (n: number) => channels[n * 2 + 1] as (event: api.SessionEventDto) => void;
const flow = () => useConnectStore.getState().active;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  resetSessionEvents();
  attempts = [];
  channels = [];
  let next = 0;
  useConnectStore.setState(useConnectStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  useHostsStore.setState({ hosts: [web, viaProfile], profiles: [withUsage(deploy, 1)], loaded: true });
  vi.mocked(api.newId).mockImplementation(() => `id-${++next}`);
  vi.mocked(api.createOutputStream).mockReturnValue(stream as unknown as api.OutputStream);
  vi.mocked(api.createEventChannel).mockImplementation((handler) => {
    channels.push(handler as (event: never) => void);
    return { channel: channels.length } as never;
  });
  vi.mocked(api.connectSsh).mockImplementation(
    () =>
      new Promise<api.ConnectedSession>((resolve, reject) => {
        attempts.push({ resolve: (sessionId) => resolve({ sessionId, info: makeSessionInfo() }), reject });
      }),
  );
  vi.mocked(api.connectAnswer).mockResolvedValue(true);
  vi.mocked(api.connectCancel).mockResolvedValue(undefined);
  vi.mocked(api.sessionClose).mockResolvedValue(undefined);
  vi.mocked(api.secretSet).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("nextRetryDelay", () => {
  it("backs off 2/4/8 s for the first three attempts, then stops", () => {
    expect([1, 2, 3, 4, 0].map(nextRetryDelay)).toEqual([2000, 4000, 8000, null, null]);
    expect(MAX_ATTEMPTS).toBe(3);
  });
});

describe("connect store", () => {
  it("opens the connect overlay on the protocol's view and reduces events", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: { type: "connect" } });
    expect(flow()).toMatchObject({ connectId: "id-1", hostId: "h-web", protocol: "ssh", attempt: 1, autoRetry: false });
    expect(api.connectSsh).toHaveBeenCalledWith("id-1", "h-web", expect.any(Number), expect.any(Number), { channel: 1 }, stream.channel, { channel: 2 });
    eventsOf(0)({ kind: "steps", steps: ["tcp", "hostKey", "auth", "channel", "shell"] });
    eventsOf(0)({ kind: "stepStarted", step: "tcp" });
    eventsOf(0)({ kind: "log", line: "debug1: Connecting to 10.0.1.12 port 22." });
    expect(flow()).toMatchObject({ current: "tcp", log: ["debug1: Connecting to 10.0.1.12 port 22."] });
    const prompt: api.Prompt = { kind: "hostKeyUnknown", host: "10.0.1.12", port: 22, info: { algorithm: "ssh-ed25519", sha256: "SHA256:x", md5: "MD5:y" }, otherKeyTypesKnown: [] };
    eventsOf(0)({ kind: "prompt", promptId: 5, prompt });
    expect(flow()?.prompt).toEqual({ promptId: 5, prompt });
    await useConnectStore.getState().answer(5, { kind: "hostKey", decision: "trust" });
    expect(api.connectAnswer).toHaveBeenCalledWith(5, { kind: "hostKey", decision: "trust" });
    expect(flow()?.prompt).toBeUndefined();
  });

  it("adds a terminal tab with the bound stream when connected", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.resolve("s-1");
    await started;
    expect(stream.bind).toHaveBeenCalledWith("s-1");
    expect(getStream("id-2")).toBe(stream);
    expect(useSessionsStore.getState().tabs).toEqual([
      expect.objectContaining({ id: "id-2", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s-1", status: "connected" }),
    ]);
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: null });
    expect(flow()).toBeNull();
    sessionEventsOf(0)({ kind: "exit", code: 0 });
    expect(useSessionsStore.getState().tabs[0]?.status).toBe("closed");
  });

  it("stores the session info and the connect time on the new tab", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 15, 14, 2));
    const started = useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.resolve("s-1");
    await started;
    expect(useSessionsStore.getState().tabs[0]).toMatchObject({
      sessionId: "s-1",
      info: makeSessionInfo(),
      connectedAt: new Date(2026, 9, 15, 14, 2).getTime(),
    });
  });

  it("applies a session exit that arrived before the connect resolved", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    // e.g. ForceCommand/nologin: the shell ends while connect_ssh is still returning.
    sessionEventsOf(0)({ kind: "exit", code: 1 });
    attempts[0]?.resolve("s-1");
    await started;
    expect(useSessionsStore.getState().tabs).toMatchObject([{ id: "id-2", sessionId: "s-1", status: "closed" }]);
    expect(pendingSessionEventCount()).toBe(0);
  });

  it("carries the error of an early session error into the tab", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    const error = { type: "io", message: "shell request refused" } as const;
    sessionEventsOf(0)({ kind: "error", error });
    attempts[0]?.resolve("s-1");
    await started;
    expect(useSessionsStore.getState().tabs).toMatchObject([{ id: "id-2", status: "error", error }]);
  });

  it("drops a buffered session event when the attempt fails", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    sessionEventsOf(0)({ kind: "exit", code: 0 });
    attempts[0]?.reject({ type: "network", kind: "refused", message: "refused" });
    await started;
    expect(pendingSessionEventCount()).toBe(0);
  });

  it("shows network errors and retries automatically with backoff (max 3)", async () => {
    vi.useFakeTimers();
    void useConnectStore.getState().start("h-web", "ssh");
    eventsOf(0)({ kind: "steps", steps: ["tcp"] });
    eventsOf(0)({ kind: "stepStarted", step: "tcp" });
    const timeout = { type: "network", kind: "timeout", message: "connect timed out" };
    attempts[0]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(0);
    expect(flow()).toMatchObject({ failed: { step: "tcp", error: timeout }, retrying: false, attempt: 1 });
    useConnectStore.getState().setAutoRetry(true);
    await vi.advanceTimersByTimeAsync(1999);
    expect(api.connectSsh).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
    expect(flow()).toMatchObject({ attempt: 2, retrying: true, failed: { error: timeout }, connectId: "id-3" });
    attempts[1]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(4000);
    expect(api.connectSsh).toHaveBeenCalledTimes(3);
    attempts[2]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(8000);
    expect(api.connectSsh).toHaveBeenCalledTimes(4);
    attempts[3]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(api.connectSsh).toHaveBeenCalledTimes(4);
    expect(flow()).toMatchObject({ attempt: 4, retrying: false });
  });

  it("retries manually, continuing the counter; auth errors are cleared while retrying", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    expect(flow()?.failed?.error.type).toBe("auth");
    void useConnectStore.getState().retry();
    expect(flow()).toMatchObject({ attempt: 2, retrying: false, failed: undefined, steps: [] });
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
  });

  it("cancels a running attempt and ignores its late outcome", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    await useConnectStore.getState().cancel();
    expect(api.connectCancel).toHaveBeenCalledWith("id-1");
    expect(flow()).toBeNull();
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", modal: null });
    attempts[0]?.resolve("s-late");
    await started;
    expect(api.sessionClose).toHaveBeenCalledWith("s-late");
    expect(useSessionsStore.getState().tabs).toEqual([]);
  });

  it("returns to the overview with a notice when the host key was rejected", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "hostKeyRejected", message: "host key rejected" });
    await settle();
    expect(flow()).toBeNull();
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", modal: null });
    expect(useNoticeStore.getState().notices).toMatchObject([
      { tone: "amber", message: "Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert." },
    ]);
  });

  it("closes silently when the user cancelled a prompt", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "cancelled" });
    await settle();
    expect(flow()).toBeNull();
    expect(useNoticeStore.getState().notices).toEqual([]);
  });

  it("answers the next password prompt with a re-entered password", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    void useConnectStore.getState().retryWithPassword("neu", false);
    await settle();
    expect(api.secretSet).not.toHaveBeenCalled();
    eventsOf(1)({ kind: "prompt", promptId: 9, prompt: { kind: "password", user: "deploy", host: "10.0.1.12", retry: false } });
    expect(api.connectAnswer).toHaveBeenCalledWith(9, { kind: "secret", value: "neu", save: false });
    expect(flow()?.prompt).toBeUndefined();
    eventsOf(1)({ kind: "prompt", promptId: 10, prompt: { kind: "password", user: "deploy", host: "10.0.1.12", retry: true } });
    expect(flow()?.prompt?.promptId).toBe(10);
  });

  it("stores a re-entered password in the login profile before retrying", async () => {
    void useConnectStore.getState().start("h-prof", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    void useConnectStore.getState().retryWithPassword("neu", true);
    await settle();
    expect(api.secretSet).toHaveBeenCalledWith({ kind: "profile", id: "p-deploy" }, "password", "neu");
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
  });

  it("ignores events of superseded attempts", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "network", kind: "refused", message: "refused" });
    await settle();
    void useConnectStore.getState().retry();
    eventsOf(0)({ kind: "steps", steps: ["jumpHost"] });
    expect(flow()?.steps).toEqual([]);
  });

  it("opens an SFTP tab after an sftp connect", async () => {
    vi.mocked(api.connectSftp).mockResolvedValue({ sessionId: "s1", info: makeSessionInfo(), version: 3, listing: { path: "/home/u", entries: [] } });
    vi.mocked(api.localHome).mockResolvedValue("/home/me");
    vi.mocked(api.localList).mockResolvedValue({ path: "/home/me", entries: [] });
    await useConnectStore.getState().start(web.id, "sftp");
    expect(useConnectStore.getState().active).toBeNull();
    expect(useUiStore.getState().view).toBe("sftp");
    expect(useSftpStore.getState().tabs).toHaveLength(1);
    expect(api.connectSsh).not.toHaveBeenCalled();
  });

  it("cancelling an sftp reconnect stays in the sftp view", async () => {
    vi.mocked(api.connectSftp).mockReturnValue(new Promise(() => {}));
    void useConnectStore.getState().start(web.id, "sftp", { sftpTabId: "t1" });
    await useConnectStore.getState().cancel();
    expect(useUiStore.getState().view).toBe("sftp");
  });

  it("a reconnect of an SFTP tab passes its session id and never closes it on a stale result", async () => {
    vi.mocked(api.localHome).mockResolvedValue("/home/me");
    vi.mocked(api.localList).mockResolvedValue({ path: "/home/me", entries: [] });
    const connected: api.SftpConnected = { sessionId: "s1", info: makeSessionInfo(), version: 3, listing: { path: "/home/u", entries: [] } };
    useSftpStore.getState().openTab("t1", web.id, connected);
    let resolve!: (c: api.SftpConnected) => void;
    vi.mocked(api.connectSftp).mockReturnValue(new Promise((r) => { resolve = r; }));
    void useConnectStore.getState().start(web.id, "sftp", { sftpTabId: "t1" });
    await settle();
    expect(vi.mocked(api.connectSftp).mock.calls[0]![2]).toBe("s1");
    await useConnectStore.getState().cancel();
    resolve(connected);
    await settle();
    expect(api.sftpClose).not.toHaveBeenCalled();
  });

  it("reports unknown hosts", async () => {
    await useConnectStore.getState().start("missing", "ssh");
    expect(flow()).toBeNull();
    expect(useNoticeStore.getState().notices).toMatchObject([{ tone: "red", message: "Der Host wurde nicht gefunden." }]);
  });
  it("never rejects: a synchronous setup failure ends up as a failed flow", async () => {
    vi.mocked(api.createOutputStream).mockImplementation(() => {
      throw new Error("channel setup failed");
    });
    await expect(useConnectStore.getState().start("h-web", "ssh")).resolves.toBeUndefined();
    expect(flow()).toMatchObject({ failed: { error: { type: "internal", message: "channel setup failed" } }, retrying: false });
  });

  it("never rejects: a synchronously throwing connectSsh ends up as a failed flow", async () => {
    vi.mocked(api.connectSsh).mockImplementation(() => {
      throw new Error("invoke unavailable");
    });
    await expect(useConnectStore.getState().start("h-web", "ssh")).resolves.toBeUndefined();
    expect(flow()?.failed?.error).toEqual({ type: "internal", message: "invoke unavailable" });
  });
  it("does not hand the re-entered password to a jump host's prompt", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    void useConnectStore.getState().retryWithPassword("neu", false);
    await settle();
    eventsOf(1)({ kind: "prompt", promptId: 20, prompt: { kind: "password", user: "admin", host: "bastion.example.net", retry: false } });
    expect(api.connectAnswer).not.toHaveBeenCalled();
    expect(flow()?.prompt?.promptId).toBe(20);
    eventsOf(1)({ kind: "prompt", promptId: 21, prompt: { kind: "password", user: "deploy", host: "10.0.1.12", retry: false } });
    expect(api.connectAnswer).toHaveBeenCalledExactlyOnceWith(21, { kind: "secret", value: "neu", save: false });
  });

  describe("auth failure at the jump host", () => {
    const bastion = makeHost({ id: "h-bastion", name: "bastion", address: "bastion.example.net", user: "admin" });
    const jumped = makeHost({ id: "h-jumped", name: "app-01", address: "10.0.2.5", user: "deploy", jumpHostId: "h-bastion" });

    const failAtJump = async () => {
      useHostsStore.setState({ hosts: [web, viaProfile, bastion, jumped] });
      void useConnectStore.getState().start("h-jumped", "ssh");
      eventsOf(0)({ kind: "steps", steps: ["jumpHost", "tcp", "hostKey", "auth", "channel", "shell"] });
      eventsOf(0)({ kind: "stepStarted", step: "jumpHost" });
      eventsOf(0)({ kind: "stepFailed", step: "jumpHost", error: { type: "auth", rejectedMethods: ["password"], message: "denied" } });
      attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
      await settle();
      expect(flow()?.failed?.step).toBe("jumpHost");
    };

    it("saves the re-entered password for the jump host", async () => {
      await failAtJump();
      void useConnectStore.getState().retryWithPassword("neu", true);
      await settle();
      expect(api.secretSet).toHaveBeenCalledExactlyOnceWith({ kind: "host", id: "h-bastion" }, "password", "neu");
    });

    it("answers only the jump host's password prompt", async () => {
      await failAtJump();
      void useConnectStore.getState().retryWithPassword("neu", false);
      await settle();
      eventsOf(1)({ kind: "prompt", promptId: 30, prompt: { kind: "password", user: "deploy", host: "10.0.2.5", retry: false } });
      expect(api.connectAnswer).not.toHaveBeenCalled();
      expect(flow()?.prompt?.promptId).toBe(30);
      eventsOf(1)({ kind: "prompt", promptId: 31, prompt: { kind: "password", user: "admin", host: "bastion.example.net", retry: false } });
      expect(api.connectAnswer).toHaveBeenCalledExactlyOnceWith(31, { kind: "secret", value: "neu", save: false });
    });
  });

  it("starts only one attempt for a double retryWithPassword", async () => {
    void useConnectStore.getState().start("h-prof", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    void useConnectStore.getState().retryWithPassword("neu", true);
    void useConnectStore.getState().retryWithPassword("neu", true);
    expect(flow()?.submitting).toBe(true);
    await settle();
    expect(api.secretSet).toHaveBeenCalledTimes(1);
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
    expect(flow()?.submitting).toBeUndefined();
  });

  it("does not retry after the flow ended while the password was being saved", async () => {
    void useConnectStore.getState().start("h-prof", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    let release = () => {};
    vi.mocked(api.secretSet).mockImplementation(() => new Promise<void>((resolve) => (release = resolve)));
    const pending = useConnectStore.getState().retryWithPassword("neu", true);
    await useConnectStore.getState().cancel();
    release();
    await pending;
    expect(api.connectSsh).toHaveBeenCalledTimes(1);
  });

  it("cancels a still-running attempt when retrying", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    void useConnectStore.getState().retry();
    expect(api.connectCancel).toHaveBeenCalledWith("id-1");
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
  });

  it("does not cancel a finished attempt when retrying", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "network", kind: "refused", message: "refused" });
    await settle();
    void useConnectStore.getState().retry();
    expect(api.connectCancel).not.toHaveBeenCalled();
  });
});

describe("connect into a pane", () => {
  const splitPicking = () => {
    useSessionsStore.getState().addTab({ id: "p0", title: "web", kind: "ssh", hostId: "h-web", sessionId: "s-0", status: "connected" });
    vi.mocked(api.newId).mockReturnValueOnce("pane-1");
    useSessionsStore.getState().split("row");
  };

  it("puts the session into the picking pane instead of a new tab", async () => {
    splitPicking();
    const started = useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    attempts[0]?.resolve("s-1");
    await started;
    expect(useSessionsStore.getState().groups).toHaveLength(1);
    const paneId = useSessionsStore.getState().groups[0]!.focusedPaneId;
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === paneId)).toMatchObject({
      kind: "ssh",
      hostId: "h-web",
      sessionId: "s-1",
      status: "connected",
      title: "web-prod-01",
    });
    expect(getStream(paneId)).toBe(stream);
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: null });
  });

  it("returns to the terminal and keeps the picker when cancelled (Review Focus 5)", async () => {
    splitPicking();
    void useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    await useConnectStore.getState().cancel();
    expect(useUiStore.getState().view).toBe("terminal");
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === "pane-1")?.status).toBe("picking");
  });

  it("closes the session when the pane was closed while connecting (Review Focus 3)", async () => {
    splitPicking();
    const started = useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    useSessionsStore.getState().closeTab("pane-1");
    attempts[0]?.resolve("s-late");
    await started;
    expect(api.sessionClose).toHaveBeenCalledWith("s-late");
    expect(useSessionsStore.getState().groups).toHaveLength(1);
    expect(getStream("pane-1")).toBeUndefined();
  });

  it("ignores a late exit of a cancelled attempt when the pane connects again (review C1)", async () => {
    splitPicking();
    void useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    await useConnectStore.getState().cancel();
    // The cancelled attempt still got a shell: it is closed, and its exit arrives late.
    attempts[0]?.resolve("s-old");
    await settle();
    expect(api.sessionClose).toHaveBeenCalledWith("s-old");
    sessionEventsOf(0)({ kind: "exit", code: 0 });
    const started = useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    attempts[1]?.resolve("s-new");
    await started;
    const paneId = useSessionsStore.getState().groups[0]!.focusedPaneId;
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === paneId)).toMatchObject({ sessionId: "s-new", status: "connected" });
  });
});
