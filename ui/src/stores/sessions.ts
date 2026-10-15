import { create } from "zustand";
import { t } from "../i18n/de";
import {
  type CoreError,
  createEventChannel,
  createOutputStream,
  type Id,
  newId,
  type SessionEventDto,
  type SessionId,
  type SessionInfo,
  sessionClose,
  sessionOpenLocal,
} from "../ipc/api";
import { errorDetails } from "../ipc/errors";
import { pushNotice } from "./notices";
import { discardEarlySessionEvent, sessionEventHandler, sessionEventPatch, takeEarlySessionEvent } from "./sessionEvents";
import { useSettingsStore } from "./settings";
import { dropStream, registerStream } from "./streams";
import { estimateTerminalSize } from "./terminalSize";
import { useUiStore } from "./ui";

export type TabKind = "ssh" | "local";
export type TabStatus = "connecting" | "connected" | "closed" | "error";

export interface Tab {
  id: string;
  title: string;
  kind: TabKind;
  hostId?: Id;
  sessionId?: SessionId;
  status: TabStatus;
  /** Why the session ended, for `status: "error"`. */
  error?: CoreError;
  /** Negotiated algorithms etc. of an SSH session (sidebar "Sitzung"). */
  info?: SessionInfo;
  /** Client time (ms) when the session connected / the local terminal opened. */
  connectedAt?: number;
  /** Current terminal size, reported by `TerminalView`. */
  cols?: number;
  rows?: number;
}

export interface SessionsState {
  tabs: Tab[];
  activeId: string | null;
  /** Appends and activates the tab; applies a session event that arrived before it existed. */
  addTab(tab: Tab): void;
  /** Removes the tab; if it was active, activates the right neighbour, else the left one. */
  closeTab(id: string): void;
  setActive(id: string): void;
  updateTab(id: string, patch: Partial<Omit<Tab, "id">>): void;
  /** Opens a local terminal tab (Ctrl+Shift+T, "+" in the tab bar); failures become a notice. */
  openLocal(): Promise<void>;
  /** Closes the tab and its backend session (best effort). */
  closeSession(id: string): Promise<void>;
}

export const useSessionsStore = create<SessionsState>()((set, get) => ({
  tabs: [],
  activeId: null,
  addTab: (tab) => {
    const early = takeEarlySessionEvent(tab.id);
    const added = early ? { ...tab, ...sessionEventPatch(early) } : tab;
    set((state) => ({ tabs: [...state.tabs, added], activeId: added.id }));
  },
  closeTab: (id) =>
    set((state) => {
      const index = state.tabs.findIndex((tab) => tab.id === id);
      if (index === -1) return state;
      const tabs = state.tabs.filter((tab) => tab.id !== id);
      if (state.activeId !== id) return { tabs };
      const neighbour = tabs[index] ?? tabs[index - 1];
      return { tabs, activeId: neighbour ? neighbour.id : null };
    }),
  setActive: (activeId) => set({ activeId }),
  updateTab: (id, patch) =>
    set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === id ? { ...tab, ...patch } : tab)) })),
  openLocal: async () => {
    const tabId = newId();
    const stream = createOutputStream();
    const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
    const { cols, rows } = estimateTerminalSize(useSettingsStore.getState().settings.terminalFontSize);
    try {
      const sessionId = await sessionOpenLocal(cols, rows, stream.channel, sessionEvents);
      stream.bind(sessionId);
      registerStream(tabId, stream);
      const count = get().tabs.filter((tab) => tab.kind === "local").length;
      const title = count === 0 ? t.terminal.localTitle : `${t.terminal.localTitle} ${count + 1}`;
      get().addTab({ id: tabId, title, kind: "local", sessionId, status: "connected", connectedAt: Date.now() });
      useUiStore.getState().setView("terminal");
    } catch (error) {
      discardEarlySessionEvent(tabId);
      pushNotice({ tone: "red", message: t.terminal.localFailed, details: errorDetails(error) });
    }
  },
  closeSession: async (id) => {
    const tab = get().tabs.find((candidate) => candidate.id === id);
    get().closeTab(id);
    dropStream(id);
    if (!tab?.sessionId) return;
    try {
      await sessionClose(tab.sessionId);
    } catch {
      // the session already ended
    }
  },
}));
