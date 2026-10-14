import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { makeHost, makeProfile, withUsage } from "../test/fixtures";
import { MAX_ATTEMPTS, nextRetryDelay, useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useNoticeStore } from "./notices";
import { useSessionsStore } from "./sessions";
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
  attempts = [];
  channels = [];
  let next = 0;
  useConnectStore.setState(useConnectStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
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
      new Promise<string>((resolve, reject) => {
        attempts.push({ resolve, reject });
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
    const prompt = { kind: "hostKeyUnknown", host: "10.0.1.12", port: 22, info: { algorithm: "ssh-ed25519", sha256: "SHA256:x", md5: "MD5:y" } } as const;
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
      { id: "id-2", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s-1", status: "connected" },
    ]);
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: null });
    expect(flow()).toBeNull();
    sessionEventsOf(0)({ kind: "exit", code: 0 });
    expect(useSessionsStore.getState().tabs[0]?.status).toBe("closed");
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
});
