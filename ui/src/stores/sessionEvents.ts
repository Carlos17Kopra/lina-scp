import { t } from "../i18n/de";
import type { SessionEventDto } from "../ipc/api";
import { type Tab, useSessionsStore } from "./sessions";
import { getStream } from "./streams";
import { setTunnelState } from "./tunnels";

/**
 * Terminal session events that arrived before their tab existed or while it still showed the pane
 * picker. The backend may report the end of a session (nologin shell, ForceCommand, refused shell,
 * invalid `$SHELL`) while `connectSsh` / `sessionOpenLocal` is still returning; `addTab` /
 * `attachSession` apply and clear the entry.
 */
const early = new Map<string, SessionEventDto>();
/** Keys whose tab was added and may still get events: an event for such a key without a tab belongs to a closed pane. */
const added = new Set<string>();

/** The tab fields a session event sets (ended, failed, reconnecting, restored, lost). */
export function sessionEventPatch(event: SessionEventDto): Partial<Omit<Tab, "id">> {
  switch (event.kind) {
    case "exit":
      return { status: "closed" };
    case "error":
      return { status: "error", error: event.error };
    case "reconnecting":
      return {
        status: "reconnecting",
        reconnect: { attempt: event.attempt, maxAttempts: event.maxAttempts, retryAt: Date.now() + event.retryInMs, reason: event.reason },
      };
    case "reconnected":
      return { status: "connected", info: event.info, connectedAt: Date.now(), reconnect: undefined, error: undefined };
    case "lost":
      return { status: "lost", error: event.reason, reconnect: undefined };
    case "tunnel":
      return {};
  }
}

/** Handler for a session's `session_events` channel: the tab shows "Sitzung beendet" afterwards. */
export function sessionEventHandler(tabId: string): (event: SessionEventDto) => void {
  return (event) => {
    if (event.kind === "tunnel") {
      setTunnelState(tabId, event.event);
      return;
    }
    const sessions = useSessionsStore.getState();
    // A picking pane has no session yet: keep the event for `attachSession`.
    if (sessions.tabs.some((tab) => tab.id === tabId && tab.status !== "picking")) {
      if (event.kind === "reconnected") {
        // Synchronously, in stream order: before the new shell's first output.
        getStream(tabId)?.inject(new TextEncoder().encode(`\r\n\u001b[2m${t.terminal.reconnecting.restored}\u001b[0m\r\n`));
      }
      sessions.updateTab(tabId, sessionEventPatch(event));
      // Nothing follows an exit or error: forget the key.
      if (event.kind === "exit" || event.kind === "error") added.delete(tabId);
    } else if (!added.delete(tabId)) early.set(tabId, event);
  };
}

/** Removes and returns the event that arrived before tab `tabId` was added. */
export function takeEarlySessionEvent(tabId: string): SessionEventDto | undefined {
  added.add(tabId);
  const event = early.get(tabId);
  early.delete(tabId);
  return event;
}

/** Forgets a buffered event of a tab that will never be added (failed or cancelled start). */
export function discardEarlySessionEvent(tabId: string): void {
  early.delete(tabId);
}

/** Test helper: number of buffered events. */
export function pendingSessionEventCount(): number {
  return early.size;
}

/** Test helper: forget all buffered events and added keys (test ids repeat across tests, real ones do not). */
export function resetSessionEvents(): void {
  early.clear();
  added.clear();
}
