import { create } from "zustand";
import { type CoreError, type Id, type PortForward, type PortForwardEvent, type SessionId, type TunnelsConnected, tunnelsClose } from "../ipc/api";
import { fireAndForget } from "./connectProgress";
import { setTunnelState } from "./tunnels";

export type PfStatus =
  | { kind: "connected" }
  | { kind: "reconnecting"; attempt: number; maxAttempts: number }
  | { kind: "lost"; error: CoreError };

/** A host's connection on the port-forwarding screen. No entry = "Getrennt". */
export interface PfEntry {
  sessionId: SessionId;
  /** The rules as they were stored at connect time. */
  forwards: PortForward[];
  status: PfStatus;
}

interface PortForwardingState {
  /** By host id. Tunnel states live in `useTunnelsStore` under the entry's session id. */
  entries: Record<Id, PfEntry>;
  connected(hostId: Id, connected: TunnelsConnected): void;
  onEvent(hostId: Id, sessionId: SessionId, event: PortForwardEvent): void;
  disconnect(hostId: Id): Promise<void>;
}

export const usePortForwardingStore = create<PortForwardingState>()((set, get) => {
  const patch = (hostId: Id, status: PfStatus) =>
    set((state) => {
      const entry = state.entries[hostId];
      return entry ? { entries: { ...state.entries, [hostId]: { ...entry, status } } } : state;
    });

  return {
    entries: {},

    connected: (hostId, { sessionId, forwards }) => {
      const old = get().entries[hostId];
      // A reconnect after "lost" is a new session: the old one would keep its listeners otherwise.
      if (old && old.sessionId !== sessionId) fireAndForget(() => tunnelsClose(old.sessionId));
      set((state) => ({ entries: { ...state.entries, [hostId]: { sessionId, forwards, status: { kind: "connected" } } } }));
    },

    onEvent: (hostId, sessionId, event) => {
      if (get().entries[hostId]?.sessionId !== sessionId) return;
      switch (event.kind) {
        case "tunnel":
          setTunnelState(sessionId, event.event);
          return;
        case "reconnecting":
          patch(hostId, { kind: "reconnecting", attempt: event.attempt, maxAttempts: event.maxAttempts });
          return;
        case "reconnected":
          patch(hostId, { kind: "connected" });
          return;
        case "lost":
          patch(hostId, { kind: "lost", error: event.error });
          return;
      }
    },

    disconnect: async (hostId) => {
      const entry = get().entries[hostId];
      if (!entry) return;
      set((state) => {
        const { [hostId]: _, ...entries } = state.entries;
        return { entries };
      });
      await tunnelsClose(entry.sessionId);
    },
  };
});
