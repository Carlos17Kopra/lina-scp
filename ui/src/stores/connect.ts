import { create } from "zustand";
import { t } from "../i18n/de";
import {
  type ConnectEvent,
  type ConnectedSession,
  type CoreError,
  connectAnswer,
  connectCancel,
  connectSftp,
  connectSsh,
  createEventChannel,
  createOutputStream,
  type Host,
  type Id,
  newId,
  type OutputStream,
  type Prompt,
  type PromptAnswer,
  type SecretOwner,
  type SessionEventDto,
  secretSet,
  sessionClose,
  type SftpEvent,
  sftpClose,
  type Step,
} from "../ipc/api";
import { hostProfile } from "../model/hostFormat";
import { applyConnectEvent, asCoreError, fireAndForget, type StepProgress } from "./connectProgress";
import { useHostsStore } from "./hosts";
import { pushNotice } from "./notices";
import { discardEarlySessionEvent, sessionEventHandler } from "./sessionEvents";
import { useSessionsStore } from "./sessions";
import { useSettingsStore } from "./settings";
import { useSftpStore } from "./sftp";
import { dropStream, registerStream } from "./streams";
import { estimateTerminalSize } from "./terminalSize";
import { useUiStore, type View } from "./ui";

export type ConnectProtocol = "ssh" | "sftp";

/** "Versuch n von 3"; also the number of automatic retries. */
export const MAX_ATTEMPTS = 3;

/** Auto-retry backoff after failed attempt `attempt`: 2 s, 4 s, 8 s, then stop (`null`). */
export function nextRetryDelay(attempt: number): number | null {
  return attempt >= 1 && attempt <= MAX_ATTEMPTS ? 1000 * 2 ** attempt : null;
}

export interface ConnectFlow extends StepProgress {
  connectId: string;
  hostId: Id;
  protocol: ConnectProtocol;
  failed?: { step: Step; error: CoreError };
  prompt?: { promptId: number; prompt: Prompt };
  /** 1-based number of the current/last attempt; retries continue the counter. */
  attempt: number;
  autoRetry: boolean;
  /** A retry after a network error is running; the error screen stays visible in its busy state. */
  retrying: boolean;
  /** `retryWithPassword` is saving/starting; guards against double submits. */
  submitting?: boolean;
  /** PTY size requested for the shell. */
  cols: number;
  rows: number;
  /** Split pane (status `picking`) that receives the session instead of a new tab. */
  paneId?: string;
  /** SFTP reconnect: the tab that receives the new session. */
  sftpTabId?: string;
}

export interface ConnectState {
  active: ConnectFlow | null;
  /**
   * `target.paneId`: connect into that picking pane; `target.sftpTabId`: reconnect that SFTP tab.
   * Cancel then stays in the terminal / SFTP view.
   */
  start(hostId: Id, protocol: ConnectProtocol, target?: { paneId: string } | { sftpTabId: string }): Promise<void>;
  answer(promptId: number, answer: PromptAnswer): Promise<void>;
  cancel(): Promise<void>;
  retry(): Promise<void>;
  /**
   * ErrorAuth: retry with a new password for the host whose credentials were rejected (the jump host
   * after a failed jump step, else the target); `save` stores it for that host (or its login profile) first.
   */
  retryWithPassword(password: string, save: boolean): Promise<void>;
  setAutoRetry(on: boolean): void;
}

let retryTimer: ReturnType<typeof setTimeout> | null = null;
/**
 * Re-entered password for the next password prompt of host `hostId` (the target, or its jump host
 * when the jump step failed). Kept out of the store state on purpose.
 */
let pendingPassword: { value: string; hostId: Id } | null = null;

/**
 * The host whose credentials a failed flow's auth error is about: the jump host when the
 * `jumpHost` step failed (`null` if it no longer exists), else the target.
 */
export function authFailureHost(flow: ConnectFlow, hosts: readonly Host[]): { host: Host | null; isJump: boolean } {
  const target = hosts.find((h) => h.id === flow.hostId) ?? null;
  if (flow.failed?.step !== "jumpHost") return { host: target, isJump: false };
  const jumpId = target?.jumpHostId;
  return { host: (jumpId ? hosts.find((h) => h.id === jumpId) : undefined) ?? null, isJump: true };
}

/** Where a cancelled/rejected flow returns to. */
function homeView(flow: ConnectFlow | null): View {
  if (flow?.paneId) return "terminal";
  if (flow?.sftpTabId) return "sftp";
  return "hosts";
}

function clearRetryTimer(): void {
  if (retryTimer !== null) clearTimeout(retryTimer);
  retryTimer = null;
}

function secretOwnerOf(hostId: Id): SecretOwner {
  const host = useHostsStore.getState().hosts.find((h) => h.id === hostId);
  return host?.auth.kind === "profile" ? { kind: "profile", id: host.auth.profileId } : { kind: "host", id: hostId };
}

export const useConnectStore = create<ConnectState>()((set, get) => {
  const isCurrent = (connectId: string) => get().active?.connectId === connectId;

  const update = (connectId: string, change: (flow: ConnectFlow) => ConnectFlow) =>
    set((state) => (state.active?.connectId === connectId ? { active: change(state.active) } : state));

  const close = (view: View) => {
    clearRetryTimer();
    pendingPassword = null;
    set({ active: null });
    const ui = useUiStore.getState();
    if (ui.modal?.type === "connect") ui.closeModal();
    ui.setView(view);
  };

  /**
   * The backend prompts with `host.address` and the effective user (host user, else profile user).
   * Only the prompt of the host the password was entered for may receive it (the target, or its
   * jump host after a failed jump step), never another hop's (fail closed).
   */
  const isPromptFor = (hostId: Id, prompt: { host: string; user: string }) => {
    const host = useHostsStore.getState().hosts.find((h) => h.id === hostId);
    if (!host) return false;
    const profile = hostProfile(host, useHostsStore.getState().profiles);
    const user = host.user?.trim() || profile?.user.trim();
    return !!user && prompt.host === host.address && prompt.user === user;
  };

  const onEvent = (connectId: string, event: ConnectEvent) => {
    if (!isCurrent(connectId)) return;
    if (event.kind === "prompt") {
      if (event.prompt.kind === "password" && pendingPassword !== null && isPromptFor(pendingPassword.hostId, event.prompt)) {
        const { value } = pendingPassword;
        pendingPassword = null;
        fireAndForget(() => connectAnswer(event.promptId, { kind: "secret", value, save: false }));
        return;
      }
      update(connectId, (flow) => ({ ...flow, prompt: { promptId: event.promptId, prompt: event.prompt } }));
      return;
    }
    update(connectId, (flow) => applyConnectEvent(flow, event));
  };

  const scheduleAutoRetry = () => {
    clearRetryTimer();
    const flow = get().active;
    if (!flow || !flow.autoRetry || flow.retrying || flow.failed?.error.type !== "network") return;
    const delay = nextRetryDelay(flow.attempt);
    if (delay === null) return;
    const { connectId } = flow;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      if (isCurrent(connectId)) void get().retry();
    }, delay);
  };

  const onFailure = (connectId: string, error: CoreError) => {
    if (!isCurrent(connectId)) return;
    pendingPassword = null;
    const home = homeView(get().active);
    if (error.type === "cancelled") {
      close(home);
      return;
    }
    if (error.type === "hostKeyRejected") {
      close(home);
      pushNotice({ tone: "amber", message: t.connect.hostKeyRejected });
      return;
    }
    update(connectId, (flow) => ({
      ...flow,
      failed: { step: flow.failedStep ?? flow.current ?? "tcp", error },
      current: undefined,
      prompt: undefined,
      retrying: false,
    }));
    scheduleAutoRetry();
  };

  const onConnected = (flow: ConnectFlow, { sessionId, info, forwards }: ConnectedSession, tabId: string, stream: OutputStream) => {
    stream.bind(sessionId);
    registerStream(tabId, stream);
    const title = useHostsStore.getState().hosts.find((h) => h.id === flow.hostId)?.name ?? flow.hostId;
    const session = { title, kind: "ssh", hostId: flow.hostId, sessionId, status: "connected", info, forwards, connectedAt: Date.now() } as const;
    const sessions = useSessionsStore.getState();
    if (!flow.paneId) sessions.addTab({ id: tabId, ...session });
    else if (!sessions.attachSession(flow.paneId, tabId, session)) {
      // The pane was closed while connecting: no stray tab, no orphaned session.
      dropStream(tabId);
      discardEarlySessionEvent(tabId);
      fireAndForget(() => sessionClose(sessionId));
    }
    close("terminal");
  };

  const runSftp = async (flow: ConnectFlow) => {
    const { connectId } = flow;
    const tabId = flow.sftpTabId ?? newId();
    let bound: string | null = null;
    const events = createEventChannel<ConnectEvent>((event) => onEvent(connectId, event));
    const existing = flow.sftpTabId ? (useSftpStore.getState().tabs.find((t) => t.id === flow.sftpTabId)?.sessionId ?? null) : null;
    // On a re-attach the backend keeps the tab's first channel; this one stays silent.
    const sftpEvents = createEventChannel<SftpEvent>((event) => {
      if (bound !== null) useSftpStore.getState().onEvent(tabId, bound, event);
    });
    const connected = await connectSftp(connectId, flow.hostId, existing, events, sftpEvents);
    if (!isCurrent(connectId)) {
      // A re-attached connection belongs to the tab's session: never close that one here.
      if (!existing) fireAndForget(() => sftpClose(connected.sessionId));
      return;
    }
    bound = connected.sessionId;
    const sftp = useSftpStore.getState();
    if (!flow.sftpTabId) sftp.openTab(tabId, flow.hostId, connected);
    else if (!sftp.attachSession(flow.sftpTabId, connected)) fireAndForget(() => sftpClose(connected.sessionId));
    close("sftp");
  };

  const run = async (flow: ConnectFlow) => {
    const { connectId } = flow;
    // Everything that can throw (setup and invoke) lives inside the try: callers fire `start`/`retry`
    // with `void`, so a failure must surface as flow state, never as a rejected promise.
    let tabId: string | null = null;
    try {
      if (flow.protocol === "sftp") return await runSftp(flow);
      tabId = newId();
      const stream = createOutputStream();
      const events = createEventChannel<ConnectEvent>((event) => onEvent(connectId, event));
      const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
      const connected = await connectSsh(connectId, flow.hostId, flow.cols, flow.rows, events, stream.channel, sessionEvents);
      if (!isCurrent(connectId)) {
        discardEarlySessionEvent(tabId);
        fireAndForget(() => sessionClose(connected.sessionId));
        return;
      }
      onConnected(flow, connected, tabId, stream);
    } catch (error) {
      if (tabId !== null) discardEarlySessionEvent(tabId);
      onFailure(connectId, asCoreError(error));
    }
  };

  return {
    active: null,

    start: async (hostId, protocol, target) => {
      if (!useHostsStore.getState().hosts.some((h) => h.id === hostId)) {
        pushNotice({ tone: "red", message: t.connect.hostMissing });
        return;
      }
      clearRetryTimer();
      pendingPassword = null;
      const previous = get().active;
      if (previous && (!previous.failed || previous.retrying)) fireAndForget(() => connectCancel(previous.connectId));
      const { cols, rows } = estimateTerminalSize(useSettingsStore.getState().settings.terminalFontSize);
      const flow: ConnectFlow = {
        connectId: newId(),
        hostId,
        protocol,
        steps: [],
        done: [],
        log: [],
        attempt: 1,
        autoRetry: false,
        retrying: false,
        cols,
        rows,
        paneId: target && "paneId" in target ? target.paneId : undefined,
        sftpTabId: target && "sftpTabId" in target ? target.sftpTabId : undefined,
      };
      set({ active: flow });
      const ui = useUiStore.getState();
      ui.setView(protocol === "ssh" ? "terminal" : "sftp");
      ui.openModal({ type: "connect" });
      await run(flow);
    },

    answer: async (promptId, answer) => {
      set((state) =>
        state.active?.prompt?.promptId === promptId ? { active: { ...state.active, prompt: undefined } } : state,
      );
      try {
        await connectAnswer(promptId, answer);
      } catch {
        // the prompt timed out or the attempt ended meanwhile
      }
    },

    cancel: async () => {
      const flow = get().active;
      if (!flow) return;
      close(homeView(flow));
      if (!flow.failed || flow.retrying) {
        try {
          await connectCancel(flow.connectId);
        } catch {
          // the attempt already finished
        }
      }
    },

    retry: async () => {
      clearRetryTimer();
      const flow = get().active;
      if (!flow) return;
      if (!flow.failed || flow.retrying) fireAndForget(() => connectCancel(flow.connectId));
      const keepError = flow.failed?.error.type === "network";
      const next: ConnectFlow = {
        ...flow,
        connectId: newId(),
        steps: [],
        done: [],
        log: [],
        current: undefined,
        failedStep: undefined,
        prompt: undefined,
        attempt: flow.attempt + 1,
        retrying: keepError,
        submitting: undefined,
        failed: keepError ? flow.failed : undefined,
      };
      set({ active: next });
      await run(next);
    },

    retryWithPassword: async (password, save) => {
      const flow = get().active;
      if (!flow || flow.submitting) return;
      // The password belongs to the host whose credentials were rejected (jump host or target).
      const subject = authFailureHost(flow, useHostsStore.getState().hosts).host;
      if (!subject) return;
      set({ active: { ...flow, submitting: true } });
      pendingPassword = { value: password, hostId: subject.id };
      if (save) {
        try {
          await secretSet(secretOwnerOf(subject.id), "password", password);
          pendingPassword = null;
        } catch {
          pushNotice({ tone: "amber", message: t.connect.secretFailed });
        }
        if (!isCurrent(flow.connectId)) {
          pendingPassword = null;
          return;
        }
      }
      await get().retry();
    },

    setAutoRetry: (on) => {
      const flow = get().active;
      if (!flow) return;
      set({ active: { ...flow, autoRetry: on } });
      if (on) scheduleAutoRetry();
      else clearRetryTimer();
    },
  };
});
