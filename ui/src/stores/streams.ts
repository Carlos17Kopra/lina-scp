import type { OutputStream } from "../ipc/api";

/**
 * Output streams by tab id. A stream is created before the session exists (connect flow, local
 * terminal) and attached by `TerminalView` once the tab is rendered; chunks are buffered meanwhile.
 */
const streams = new Map<string, OutputStream>();

export function registerStream(tabId: string, stream: OutputStream): void {
  streams.set(tabId, stream);
}

export function getStream(tabId: string): OutputStream | undefined {
  return streams.get(tabId);
}

export function dropStream(tabId: string): void {
  streams.delete(tabId);
}
