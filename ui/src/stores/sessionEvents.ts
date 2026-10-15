import type { SessionEventDto } from "../ipc/api";
import { type Tab, useSessionsStore } from "./sessions";

/**
 * Terminal session events that arrived before their tab existed or while it still showed the pane
 * picker. The backend may report the end of a session (nologin shell, ForceCommand, refused shell,
 * invalid `$SHELL`) while `connectSsh` / `sessionOpenLocal` is still returning; `addTab` /
 * `attachSession` apply and clear the entry.
 */
const early = new Map<string, SessionEventDto>();

/** The tab fields a session event sets: "Sitzung beendet" (exit) or "… mit Fehler beendet" (error). */
export function sessionEventPatch(event: SessionEventDto): Partial<Omit<Tab, "id">> {
  return event.kind === "exit" ? { status: "closed" } : { status: "error", error: event.error };
}

/** Handler for a session's `session_events` channel: the tab shows "Sitzung beendet" afterwards. */
export function sessionEventHandler(tabId: string): (event: SessionEventDto) => void {
  return (event) => {
    const sessions = useSessionsStore.getState();
    // A picking pane has no session yet: keep the event for `attachSession`.
    if (sessions.tabs.some((tab) => tab.id === tabId && tab.status !== "picking")) {
      sessions.updateTab(tabId, sessionEventPatch(event));
    } else early.set(tabId, event);
  };
}

/** Removes and returns the event that arrived before tab `tabId` was added. */
export function takeEarlySessionEvent(tabId: string): SessionEventDto | undefined {
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
