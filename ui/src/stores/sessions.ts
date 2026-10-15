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
import {
  type Layout,
  leaf,
  MAX_PANES,
  neighbour,
  paneIds,
  removePane,
  renamePane,
  type SplitDir,
  type SplitPath,
  setRatio as setLayoutRatio,
  splitPane,
} from "../screens/terminal/layout";
import { pushNotice } from "./notices";
import { discardEarlySessionEvent, sessionEventHandler, sessionEventPatch, takeEarlySessionEvent } from "./sessionEvents";
import { useSettingsStore } from "./settings";
import { dropStream, registerStream } from "./streams";
import { estimateTerminalSize } from "./terminalSize";
import { useUiStore } from "./ui";

export type TabKind = "ssh" | "local";
/** `picking`: a new split pane that shows the host/local picker and has no session yet. */
export type TabStatus = "picking" | "connecting" | "connected" | "closed" | "error";

/** One terminal pane = one session. Grouped into UI tabs by `TabGroup`. */
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
  /** Picking panes: the pane it was split from (offered as "Gleicher Host"). */
  origin?: { kind: TabKind; hostId?: Id };
}

/** A UI tab: the split layout of its panes. Its id is the id of its first pane. */
export interface TabGroup {
  id: string;
  layout: Layout;
  focusedPaneId: string;
  broadcast: boolean;
}

export interface SessionsState {
  /** All panes of all groups. */
  tabs: Tab[];
  groups: TabGroup[];
  /** Active group id. */
  activeId: string | null;
  /** Adds the pane as a new single-pane group and activates it; applies an early session event. */
  addTab(tab: Tab): void;
  /** Removes the pane from its group; the group goes with its last pane (then a neighbour group activates). */
  closeTab(id: string): void;
  setActive(groupId: string): void;
  updateTab(id: string, patch: Partial<Omit<Tab, "id">>): void;
  /** Opens a local terminal as a new tab, or inside picking pane `target.paneId`; failures become a notice. */
  openLocal(target?: { paneId: string }): Promise<void>;
  /** Removes the pane and closes its backend session (best effort). */
  closeSession(id: string): Promise<void>;
  /** Closes every pane (and session) of the group. */
  closeGroup(groupId: string): Promise<void>;
  /** Splits the focused pane of the active group; the new pane picks what to open. No-op at `MAX_PANES`. */
  split(dir: SplitDir): void;
  focusPane(paneId: string): void;
  focusNeighbour(step: 1 | -1): void;
  setRatio(groupId: string, path: SplitPath, ratio: number): void;
  /** Toggles broadcast of the active group; only with at least two panes. */
  toggleBroadcast(): void;
  /** Gives a picking pane its session. False if the pane is gone or not picking (caller closes the session). */
  attachSession(paneId: string, patch: Partial<Omit<Tab, "id">>): boolean;
  /**
   * Swaps the pane for a fresh picking pane (new id, same place) and closes the old session. A late event
   * of the old session then targets the old id and cannot end whatever connects in the new pane.
   */
  replacePane(paneId: string): string;
}

const singleGroup = (paneId: string): TabGroup => ({ id: paneId, layout: leaf(paneId), focusedPaneId: paneId, broadcast: false });

const groupOfPane = (groups: readonly TabGroup[], paneId: string) =>
  groups.find((group) => paneIds(group.layout).includes(paneId));

export function activeGroup(state: Pick<SessionsState, "groups" | "activeId">): TabGroup | undefined {
  return state.groups.find((group) => group.id === state.activeId);
}

/** The focused pane of the active group (the sidebar's subject). */
export function focusedPane(state: Pick<SessionsState, "groups" | "activeId" | "tabs">): Tab | undefined {
  const group = activeGroup(state);
  return group ? state.tabs.find((tab) => tab.id === group.focusedPaneId) : undefined;
}

/** Sessions that also receive `paneId`'s input: only while it is the focused pane of a broadcasting group. */
export function broadcastPeers(paneId: string): SessionId[] {
  const { tabs, groups } = useSessionsStore.getState();
  const group = groups.find((candidate) => candidate.broadcast && candidate.focusedPaneId === paneId);
  if (!group) return [];
  const ids = paneIds(group.layout);
  return tabs
    .filter((tab) => tab.id !== paneId && ids.includes(tab.id) && tab.status === "connected" && tab.sessionId)
    .map((tab) => tab.sessionId as SessionId);
}

function withoutPane(state: SessionsState, paneId: string): Partial<SessionsState> {
  const tabs = state.tabs.filter((tab) => tab.id !== paneId);
  const index = state.groups.findIndex((group) => paneIds(group.layout).includes(paneId));
  const group = state.groups[index];
  if (!group) return { tabs };
  const layout = removePane(group.layout, paneId);
  if (layout) {
    const focusedPaneId = group.focusedPaneId === paneId ? neighbour(group.layout, paneId, -1) : group.focusedPaneId;
    const broadcast = group.broadcast && paneIds(layout).length > 1;
    return { tabs, groups: state.groups.map((g) => (g === group ? { ...g, layout, focusedPaneId, broadcast } : g)) };
  }
  const groups = state.groups.filter((g) => g !== group);
  if (state.activeId !== group.id) return { tabs, groups };
  const next = groups[index] ?? groups[index - 1];
  return { tabs, groups, activeId: next ? next.id : null };
}

async function closeQuietly(sessionId: SessionId | undefined): Promise<void> {
  if (!sessionId) return;
  try {
    await sessionClose(sessionId);
  } catch {
    // the session already ended
  }
}

export const useSessionsStore = create<SessionsState>()((set, get) => {
  const updateGroup = (groupId: string, change: (group: TabGroup) => TabGroup) =>
    set((state) => ({ groups: state.groups.map((group) => (group.id === groupId ? change(group) : group)) }));

  return {
    tabs: [],
    groups: [],
    activeId: null,
    addTab: (tab) => {
      const early = takeEarlySessionEvent(tab.id);
      const added = early ? { ...tab, ...sessionEventPatch(early) } : tab;
      set((state) => ({ tabs: [...state.tabs, added], groups: [...state.groups, singleGroup(added.id)], activeId: added.id }));
    },
    closeTab: (id) => set((state) => withoutPane(state, id)),
    setActive: (activeId) => set({ activeId }),
    updateTab: (id, patch) =>
      set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === id ? { ...tab, ...patch } : tab)) })),
    openLocal: async (target) => {
      const tabId = target?.paneId ?? newId();
      const stream = createOutputStream();
      const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
      const { cols, rows } = estimateTerminalSize(useSettingsStore.getState().settings.terminalFontSize);
      try {
        const sessionId = await sessionOpenLocal(cols, rows, stream.channel, sessionEvents);
        stream.bind(sessionId);
        registerStream(tabId, stream);
        const count = get().tabs.filter((tab) => tab.kind === "local" && tab.status !== "picking").length;
        const title = count === 0 ? t.terminal.localTitle : `${t.terminal.localTitle} ${count + 1}`;
        const opened = { title, kind: "local", sessionId, status: "connected", connectedAt: Date.now() } as const;
        if (!target) get().addTab({ id: tabId, ...opened });
        else if (!get().attachSession(tabId, opened)) {
          dropStream(tabId);
          discardEarlySessionEvent(tabId);
          await closeQuietly(sessionId);
          return;
        }
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
      await closeQuietly(tab?.sessionId);
    },
    closeGroup: async (groupId) => {
      const group = get().groups.find((candidate) => candidate.id === groupId);
      if (!group) return;
      await Promise.all(paneIds(group.layout).map((paneId) => get().closeSession(paneId)));
    },
    split: (dir) => {
      const state = get();
      const group = activeGroup(state);
      if (!group || paneIds(group.layout).length >= MAX_PANES) return;
      const source = state.tabs.find((tab) => tab.id === group.focusedPaneId);
      const origin = source?.status === "picking" ? source.origin : source && { kind: source.kind, hostId: source.hostId };
      const paneId = newId();
      const pane: Tab = { id: paneId, title: t.terminal.panes.newPane, kind: origin?.kind ?? "local", status: "picking", origin };
      set((current) => ({
        tabs: [...current.tabs, pane],
        groups: current.groups.map((g) =>
          g.id === group.id ? { ...g, layout: splitPane(g.layout, g.focusedPaneId, dir, paneId), focusedPaneId: paneId } : g,
        ),
      }));
    },
    focusPane: (paneId) => {
      const group = groupOfPane(get().groups, paneId);
      if (!group || group.focusedPaneId === paneId) return;
      updateGroup(group.id, (g) => ({ ...g, focusedPaneId: paneId }));
    },
    focusNeighbour: (step) => {
      const group = activeGroup(get());
      if (group) get().focusPane(neighbour(group.layout, group.focusedPaneId, step));
    },
    setRatio: (groupId, path, ratio) => updateGroup(groupId, (g) => ({ ...g, layout: setLayoutRatio(g.layout, path, ratio) })),
    toggleBroadcast: () => {
      const group = activeGroup(get());
      if (!group || (!group.broadcast && paneIds(group.layout).length < 2)) return;
      updateGroup(group.id, (g) => ({ ...g, broadcast: !g.broadcast }));
    },
    attachSession: (paneId, patch) => {
      const pane = get().tabs.find((tab) => tab.id === paneId);
      if (pane?.status !== "picking") return false;
      const early = takeEarlySessionEvent(paneId);
      get().updateTab(paneId, { ...patch, origin: undefined, ...(early ? sessionEventPatch(early) : {}) });
      return true;
    },
    replacePane: (paneId) => {
      const pane = get().tabs.find((tab) => tab.id === paneId);
      if (!pane) return paneId;
      const freshId = newId();
      const fresh: Tab = {
        id: freshId,
        title: pane.title,
        kind: pane.kind,
        status: "picking",
        origin: { kind: pane.kind, hostId: pane.hostId },
      };
      set((state) => ({
        tabs: state.tabs.map((tab) => (tab.id === paneId ? fresh : tab)),
        groups: state.groups.map((g) =>
          paneIds(g.layout).includes(paneId)
            ? { ...g, layout: renamePane(g.layout, paneId, freshId), focusedPaneId: g.focusedPaneId === paneId ? freshId : g.focusedPaneId }
            : g,
        ),
      }));
      dropStream(paneId);
      void closeQuietly(pane.sessionId);
      return freshId;
    },
  };
});
