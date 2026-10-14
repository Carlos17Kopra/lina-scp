import { create } from "zustand";
import type { Id, SessionId } from "../ipc/api";

export type TabKind = "ssh" | "local";
export type TabStatus = "connecting" | "connected" | "closed" | "error";

export interface Tab {
  id: string;
  title: string;
  kind: TabKind;
  hostId?: Id;
  sessionId?: SessionId;
  status: TabStatus;
}

export interface SessionsState {
  tabs: Tab[];
  activeId: string | null;
  /** Appends and activates the tab. */
  addTab(tab: Tab): void;
  /** Removes the tab; if it was active, activates the right neighbour, else the left one. */
  closeTab(id: string): void;
  setActive(id: string): void;
  updateTab(id: string, patch: Partial<Omit<Tab, "id">>): void;
  /** Opens a local terminal tab (Ctrl+Shift+T, "+" in the tab bar). Implemented in Task 22. */
  openLocal(): Promise<void>;
}

export const useSessionsStore = create<SessionsState>()((set) => ({
  tabs: [],
  activeId: null,
  addTab: (tab) => set((state) => ({ tabs: [...state.tabs, tab], activeId: tab.id })),
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
  openLocal: async () => undefined,
}));
