import { create } from "zustand";
import { t } from "../i18n/de";
import {
  type ConnectEvent,
  type CoreError,
  connectAnswer,
  connectCancel,
  connectSsh,
  createEventChannel,
  createOutputStream,
  type Id,
  newId,
  type OutputStream,
  type Prompt,
  type PromptAnswer,
  type SecretOwner,
  type SessionEventDto,
  secretSet,
  sessionClose,
  type Step,
} from "../ipc/api";
import { applyConnectEvent, asCoreError, fireAndForget, type StepProgress } from "./connectProgress";
import { useHostsStore } from "./hosts";
import { pushNotice } from "./notices";
import { sessionEventHandler } from "./sessionEvents";
import { useSessionsStore } from "./sessions";
import { useSettingsStore } from "./settings";
import { registerStream } from "./streams";
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
  /** PTY size requested for the shell. */
  cols: number;
  rows: number;
}

export interface ConnectState {
  active: ConnectFlow | null;
  start(hostId: Id, protocol: ConnectProtocol): Promise<void>;
  answer(promptId: number, answer: PromptAnswer): Promise<void>;
  cancel(): Promise<void>;
  retry(): Promise<void>;
  /** ErrorAuth: retry with a new password; `save` stores it for the host (or its login profile) first. */
  retryWithPassword(password: string, save: boolean): Promise<void>;
  setAutoRetry(on: boolean): void;
}

let retryTimer: ReturnType<typeof setTimeout> | null = null;
/** Re-entered password for the next password prompt (kept out of the store state on purpose). */
let pendingPassword: string | null = null;

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

  const onEvent = (connectId: string, event: ConnectEvent) => {
    if (!isCurrent(connectId)) return;
    if (event.kind === "prompt") {
      if (event.prompt.kind === "password" && pendingPassword !== null) {
        const value = pendingPassword;
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
    if (error.type === "cancelled") {
      close("hosts");
      return;
    }
    if (error.type === "hostKeyRejected") {
      close("hosts");
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

  const onConnected = (flow: ConnectFlow, sessionId: string, tabId: string, stream: OutputStream) => {
    stream.bind(sessionId);
    registerStream(tabId, stream);
    const title = useHostsStore.getState().hosts.find((h) => h.id === flow.hostId)?.name ?? flow.hostId;
    useSessionsStore.getState().addTab({ id: tabId, title, kind: "ssh", hostId: flow.hostId, sessionId, status: "connected" });
    close("terminal");
  };

  const run = async (flow: ConnectFlow) => {
    const { connectId } = flow;
    // Everything that can throw (setup and invoke) lives inside the try: callers fire `start`/`retry`
    // with `void`, so a failure must surface as flow state, never as a rejected promise.
    try {
      const tabId = newId();
      const stream = createOutputStream();
      const events = createEventChannel<ConnectEvent>((event) => onEvent(connectId, event));
      const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
      const sessionId = await connectSsh(connectId, flow.hostId, flow.cols, flow.rows, events, stream.channel, sessionEvents);
      if (!isCurrent(connectId)) {
        fireAndForget(() => sessionClose(sessionId));
        return;
      }
      onConnected(flow, sessionId, tabId, stream);
    } catch (error) {
      onFailure(connectId, asCoreError(error));
    }
  };

  return {
    active: null,

    start: async (hostId, protocol) => {
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
      close("hosts");
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
        failed: keepError ? flow.failed : undefined,
      };
      set({ active: next });
      await run(next);
    },

    retryWithPassword: async (password, save) => {
      const flow = get().active;
      if (!flow) return;
      pendingPassword = password;
      if (save) {
        try {
          await secretSet(secretOwnerOf(flow.hostId), "password", password);
          pendingPassword = null;
        } catch {
          pushNotice({ tone: "amber", message: t.connect.secretFailed });
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
