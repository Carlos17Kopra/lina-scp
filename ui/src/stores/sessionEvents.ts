import type { SessionEventDto } from "../ipc/api";
import { useSessionsStore } from "./sessions";

/** Handler for a session's `session_events` channel: the tab shows "Sitzung beendet" afterwards. */
export function sessionEventHandler(tabId: string): (event: SessionEventDto) => void {
  return (event) => {
    useSessionsStore.getState().updateTab(tabId, { status: event.kind === "exit" ? "closed" : "error" });
  };
}
