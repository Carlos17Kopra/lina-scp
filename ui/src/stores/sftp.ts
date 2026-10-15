import { create } from "zustand";
import {
  type CoreError,
  type Entry,
  type Id,
  localDelete,
  localHome,
  localList,
  localMkdir,
  localRename,
  localTrash,
  type Listing,
  type SessionInfo,
  type SftpConnected,
  sftpClose,
  sftpDelete,
  sftpList,
  sftpMkdir,
  sftpRename,
} from "../ipc/api";
import { isCoreError } from "../ipc/errors";
import { type Sort, type SortKey, visibleEntries } from "../screens/sftp/entries";
import { fireAndForget } from "./connectProgress";
import { useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";

export type Side = "local" | "remote";

export interface PaneState {
  path: string;
  entries: Entry[];
  loading: boolean;
  error?: CoreError;
  selection: string[];
  cursor: string | null;
  anchor: string | null;
  sort: Sort;
  showHidden: boolean;
  editingPath: boolean;
  /** Increments per navigate; a response for an older token is dropped. */
  token: number;
}

export interface SftpTab {
  id: string;
  hostId: Id;
  sessionId: string;
  title: string;
  status: "connected" | "lost";
  lostError?: CoreError;
  info: SessionInfo;
  version: number;
  panes: Record<Side, PaneState>;
  active: Side;
}

export interface SftpState {
  tabs: SftpTab[];
  activeTabId: string | null;
  openTab(tabId: string, hostId: Id, connected: SftpConnected): void;
  attachSession(tabId: string, connected: SftpConnected): boolean;
  closeTab(tabId: string): void;
  setActiveTab(tabId: string): void;
  setActivePane(tabId: string, side: Side): void;
  markLost(tabId: string, sessionId: string, error: CoreError): void;
  reconnect(tabId: string): void;
  navigate(tabId: string, side: Side, path: string, focus?: string): Promise<void>;
  reload(tabId: string, side: Side, focus?: string): Promise<void>;
  setSort(tabId: string, side: Side, key: SortKey): void;
  toggleHidden(tabId: string, side: Side): void;
  setEditingPath(tabId: string, side: Side, on: boolean): void;
  select(tabId: string, side: Side, patch: Pick<PaneState, "selection" | "cursor" | "anchor">): void;
  mkdir(tabId: string, side: Side, name: string): Promise<void>;
  rename(tabId: string, side: Side, from: string, to: string): Promise<void>;
  remove(tabId: string, side: Side, names: string[], mode: "trash" | "permanent"): Promise<void>;
}

const asError = (error: unknown): CoreError =>
  isCoreError(error) ? error : { type: "internal", message: String(error) };

function pane(listing: Listing | null): PaneState {
  return {
    path: listing?.path ?? "",
    entries: listing?.entries ?? [],
    loading: listing === null,
    selection: [],
    cursor: null,
    anchor: null,
    sort: { key: "name", dir: "asc" },
    showHidden: false,
    editingPath: false,
    token: 0,
  };
}

export const useSftpStore = create<SftpState>()((set, get) => {
  const findTab = (tabId: string) => get().tabs.find((tab) => tab.id === tabId);
  const patchTab = (tabId: string, change: (tab: SftpTab) => Partial<SftpTab>) =>
    set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === tabId ? { ...tab, ...change(tab) } : tab)) }));
  const patchPane = (tabId: string, side: Side, change: (p: PaneState) => Partial<PaneState>) =>
    patchTab(tabId, (tab) => ({ panes: { ...tab.panes, [side]: { ...tab.panes[side], ...change(tab.panes[side]) } } }));

  const list = (tab: SftpTab, side: Side, path: string) =>
    side === "remote" ? sftpList(tab.sessionId, path) : localList(path);

  /** Cursor after a reload: `focus` if present, else the old cursor, else its next visible neighbour. */
  const cursorAfter = (before: PaneState, entries: Entry[], focus?: string): string | null => {
    const names = new Set(entries.map((e) => e.name));
    if (focus && names.has(focus)) return focus;
    if (before.cursor && names.has(before.cursor)) return before.cursor;
    const old = visibleEntries(before.entries, before.showHidden, before.sort).map((e) => e.name);
    const index = before.cursor ? old.indexOf(before.cursor) : -1;
    for (const name of [...old.slice(index + 1), ...old.slice(0, Math.max(index, 0)).reverse()]) {
      if (names.has(name)) return name;
    }
    return null;
  };

  const titleFor = (hostId: Id) => {
    const name = useHostsStore.getState().hosts.find((h) => h.id === hostId)?.name ?? hostId;
    const taken = new Set(get().tabs.map((tab) => tab.title));
    let n = 1;
    const label = (i: number) => (i === 1 ? name : `${name} ${i}`);
    while (taken.has(label(n))) n++;
    return label(n);
  };

  const loadLocalHome = async (tabId: string, hostId: Id) => {
    try {
      await get().navigate(tabId, "local", await localHome(hostId));
    } catch (error) {
      patchPane(tabId, "local", () => ({ loading: false, error: asError(error) }));
    }
  };

  return {
    tabs: [],
    activeTabId: null,

    openTab: (tabId, hostId, connected) => {
      const tab: SftpTab = {
        id: tabId,
        hostId,
        sessionId: connected.sessionId,
        title: titleFor(hostId),
        status: "connected",
        info: connected.info,
        version: connected.version,
        panes: { local: pane(null), remote: pane(connected.listing) },
        active: "remote",
      };
      set((state) => ({ tabs: [...state.tabs, tab], activeTabId: tabId }));
      void loadLocalHome(tabId, hostId);
    },

    attachSession: (tabId, connected) => {
      const tab = findTab(tabId);
      if (!tab) return false;
      const lastPath = tab.panes.remote.path;
      patchTab(tabId, () => ({ sessionId: connected.sessionId, status: "connected", lostError: undefined, info: connected.info, version: connected.version }));
      patchPane(tabId, "remote", () => ({ path: connected.listing.path, entries: connected.listing.entries, error: undefined }));
      // Back to where the user was; if that is gone, the start directory stays.
      if (lastPath && lastPath !== connected.listing.path) void get().navigate(tabId, "remote", lastPath);
      return true;
    },

    closeTab: (tabId) => {
      const tab = findTab(tabId);
      if (!tab) return;
      fireAndForget(() => sftpClose(tab.sessionId));
      set((state) => {
        const tabs = state.tabs.filter((x) => x.id !== tabId);
        const activeTabId = state.activeTabId === tabId ? (tabs.at(-1)?.id ?? null) : state.activeTabId;
        return { tabs, activeTabId };
      });
    },

    setActiveTab: (tabId) => set({ activeTabId: tabId }),
    setActivePane: (tabId, side) => patchTab(tabId, () => ({ active: side })),

    markLost: (tabId, sessionId, error) => {
      if (findTab(tabId)?.sessionId !== sessionId) return;
      patchTab(tabId, () => ({ status: "lost", lostError: error }));
    },

    reconnect: (tabId) => {
      const tab = findTab(tabId);
      if (!tab) return;
      fireAndForget(() => sftpClose(tab.sessionId));
      void useConnectStore.getState().start(tab.hostId, "sftp", { sftpTabId: tabId });
    },

    navigate: async (tabId, side, path, focus) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const token = tab.panes[side].token + 1;
      patchPane(tabId, side, () => ({ loading: true, token, editingPath: false }));
      try {
        const listing = await list(tab, side, path);
        const now = findTab(tabId)?.panes[side];
        if (!now || now.token !== token) return;
        const moved = listing.path !== now.path;
        patchPane(tabId, side, (p) => ({
          path: listing.path,
          entries: listing.entries,
          loading: false,
          error: undefined,
          selection: moved ? [] : p.selection.filter((n) => listing.entries.some((e) => e.name === n)),
          cursor: moved ? (focus ?? null) : cursorAfter(p, listing.entries, focus),
          anchor: moved ? null : p.anchor,
        }));
      } catch (error) {
        if (findTab(tabId)?.panes[side].token !== token) return;
        patchPane(tabId, side, () => ({ loading: false, error: asError(error) }));
      }
    },

    reload: (tabId, side, focus) => {
      const tab = findTab(tabId);
      return tab ? get().navigate(tabId, side, tab.panes[side].path, focus) : Promise.resolve();
    },

    setSort: (tabId, side, key) =>
      patchPane(tabId, side, (p) => ({ sort: { key, dir: p.sort.key === key && p.sort.dir === "asc" ? "desc" : "asc" } })),
    toggleHidden: (tabId, side) => patchPane(tabId, side, (p) => ({ showHidden: !p.showHidden })),
    setEditingPath: (tabId, side, on) => patchPane(tabId, side, () => ({ editingPath: on })),
    select: (tabId, side, patch) => patchPane(tabId, side, () => patch),

    mkdir: async (tabId, side, name) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const dir = tab.panes[side].path;
      await (side === "remote" ? sftpMkdir(tab.sessionId, dir, name) : localMkdir(dir, name));
      await get().reload(tabId, side, name);
    },

    rename: async (tabId, side, from, to) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const dir = tab.panes[side].path;
      await (side === "remote" ? sftpRename(tab.sessionId, dir, from, to) : localRename(dir, from, to));
      await get().reload(tabId, side, to);
    },

    remove: async (tabId, side, names, mode) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const dir = tab.panes[side].path;
      try {
        if (side === "remote") await sftpDelete(tab.sessionId, dir, names);
        else if (mode === "trash") await localTrash(dir, names);
        else await localDelete(dir, names);
        patchPane(tabId, side, () => ({ selection: [] }));
        await get().reload(tabId, side);
      } catch (error) {
        await get().reload(tabId, side);
        patchPane(tabId, side, () => ({ error: asError(error) }));
      }
    },
  };
});
