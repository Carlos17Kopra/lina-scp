import type { DiffStatus, Difference, FileStamp, NodeInfo, SyncItem } from "../../ipc/api";

export type SyncDirection = "upload" | "download" | "both";
export type SyncAction = "upload" | "download" | "deleteLocal" | "deleteRemote";

export interface PlannedRow {
  diff: Difference;
  /** `null`: skipped for `reason`. */
  action: SyncAction | null;
  reason: DiffStatus | null;
}

type Table = Record<DiffStatus, SyncAction | null>;

const TABLES: Record<SyncDirection, (deleteExtra: boolean) => Table> = {
  upload: (del) => ({
    onlyLocal: "upload", onlyRemote: del ? "deleteRemote" : null, localNewer: "upload",
    remoteNewer: null, sizeDiffers: "upload", typeMismatch: null,
  }),
  download: (del) => ({
    onlyLocal: del ? "deleteLocal" : null, onlyRemote: "download", localNewer: null,
    remoteNewer: "download", sizeDiffers: "download", typeMismatch: null,
  }),
  // Both ways never deletes; equal times with different sizes have no winner.
  both: () => ({
    onlyLocal: "upload", onlyRemote: "download", localNewer: "upload",
    remoteNewer: "download", sizeDiffers: null, typeMismatch: null,
  }),
};

export function planSync(diffs: readonly Difference[], direction: SyncDirection, deleteExtra: boolean): PlannedRow[] {
  const table = TABLES[direction](deleteExtra && direction !== "both");
  return diffs.map((diff) => {
    const action = table[diff.status];
    return { diff, action, reason: action ? null : diff.status };
  });
}

/** What the compare saw at the target: a file's stamp; nothing for a directory or a missing target. */
const stampOf = (node: NodeInfo | null): FileStamp | null => (node && !node.dir ? { size: node.size, mtime: node.mtime } : null);

export function requestsFor(rows: readonly PlannedRow[]): Record<SyncAction, SyncItem[]> {
  const out: Record<SyncAction, SyncItem[]> = { upload: [], download: [], deleteLocal: [], deleteRemote: [] };
  for (const { diff, action } of rows) {
    if (!action) continue;
    const target = action === "upload" || action === "deleteRemote" ? diff.remote : diff.local;
    out[action].push({ rel: diff.rel, expected: stampOf(target) });
  }
  return out;
}
