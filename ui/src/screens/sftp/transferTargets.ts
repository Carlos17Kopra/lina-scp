import type { Direction } from "../../ipc/api";
import type { Side, SftpTab } from "../../stores/sftp";
import { isDirLike, joinPath } from "./entries";

export interface TransferRequest {
  direction: Direction;
  sources: string[];
  targetDir: string;
}

/** What a transfer of `names` from pane `from` means; null if nothing usable or not connected. */
export function requestFor(tab: SftpTab, from: Side, names: string[], dropDir: string | null = null): TransferRequest | null {
  if (tab.status !== "connected") return null;
  const pane = tab.panes[from];
  const usable = names.filter((n) => n !== ".." && pane.entries.some((e) => e.name === n && !e.nameLossy));
  if (usable.length === 0) return null;
  const to: Side = from === "local" ? "remote" : "local";
  return {
    direction: from === "local" ? "upload" : "download",
    sources: usable.map((n) => joinPath(pane.path, n)),
    targetDir: dropDir ?? tab.panes[to].path,
  };
}

/** Target folder of a drop on `rowName` (a directory row) or on the pane background. */
export function dropDirAt(tab: SftpTab, side: Side, rowName: string | null): string {
  const pane = tab.panes[side];
  const entry = rowName ? pane.entries.find((e) => e.name === rowName) : undefined;
  return entry && isDirLike(entry) && !entry.nameLossy ? joinPath(pane.path, entry.name) : pane.path;
}
