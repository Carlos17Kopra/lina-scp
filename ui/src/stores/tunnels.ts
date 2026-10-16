import { create } from "zustand";
import type { Id, TunnelEvent, TunnelState } from "../ipc/api";

/**
 * Tunnel states per session-event key (= tab id). Kept apart from the tabs: autostart events can arrive before
 * the tab exists, and the one-slot early-event buffer of `sessionEvents` must keep the session's end for it.
 */
interface TunnelsState {
  states: Record<string, Record<Id, TunnelState>>;
}

// ponytail: entries of closed tabs stay (a few bytes each); drop them in closeTab if that ever matters.
export const useTunnelsStore = create<TunnelsState>(() => ({ states: {} }));

export function setTunnelState(key: string, event: TunnelEvent): void {
  useTunnelsStore.setState((state) => ({
    states: { ...state.states, [key]: { ...state.states[key], [event.ruleId]: event.state } },
  }));
}
