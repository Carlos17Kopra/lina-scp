import type { DiffStatus, Difference, FileStamp, NodeInfo, SyncItem } from "../../ipc/api";

export type SyncDirection = "upload" | "download" | "both";
export type SyncAction = "upload" | "download" | "deleteLocal" | "deleteRemote";

/** `hidden`: a folder to delete holds entries the preview does not show (excluded, links, special files). */
export type SkipReason = DiffStatus | "hidden";

export interface PlannedRow {
  diff: Difference;
  /** `null`: skipped for `reason`. */
  action: SyncAction | null;
  reason: SkipReason | null;
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
    const deletes = action === "deleteLocal" || action === "deleteRemote";
    if (deletes && diff.contents?.hidden) return { diff, action: null, reason: "hidden" };
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
    const deletes = action === "deleteLocal" || action === "deleteRemote";
    out[action].push({ rel: diff.rel, expected: stampOf(target), tree: deletes && diff.contents ? diff.contents.entries : null });
  }
  return out;
}
