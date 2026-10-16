import { create } from "zustand";
import {
  type CancelTarget,
  type Decision,
  type Direction,
  type FileStamp,
  type Job,
  type JobState,
  type TransferEvent,
  transferCancel,
  transferClear,
  transferEnqueue,
  transferResolve,
  transferRetry,
} from "../ipc/api";
import { fireAndForget } from "./connectProgress";
import { useSftpStore } from "./sftp";

export type TransferView = "queue" | "done" | "failed" | "edits";
export interface Sample { at: number; done: number }
export interface ConflictInfo { jobId: number; source: FileStamp; target: FileStamp }
export interface TabTransfers {
  jobs: Record<number, Job>;
  order: number[];
  conflicts: ConflictInfo[];
  samples: Record<number, Sample[]>;
  view: TransferView;
  collapsed: boolean;
}

const SPEED_WINDOW_MS = 2000;

export function viewOf(state: JobState): TransferView {
  if (state === "failed") return "failed";
  return state === "done" || state === "skipped" || state === "cancelled" ? "done" : "queue";
}

export function openCount(t: TabTransfers | undefined): number {
  return t ? t.order.filter((id) => viewOf(t.jobs[id]!.state) === "queue").length : 0;
}

/** Bytes per second over the samples of the last two seconds; `null` without two samples. */
export function speedOf(samples: Sample[], now: number): number | null {
  const recent = samples.filter((s) => now - s.at <= SPEED_WINDOW_MS);
  if (recent.length < 2) return null;
  const first = recent[0]!;
  const last = recent.at(-1)!;
  return last.at > first.at ? ((last.done - first.done) * 1000) / (last.at - first.at) : null;
}

const empty = (): TabTransfers => ({ jobs: {}, order: [], conflicts: [], samples: {}, view: "queue", collapsed: false });

interface TransfersState {
  byTab: Record<string, TabTransfers>;
  onEvent(tabId: string, event: TransferEvent, now?: number): void;
  enqueue(tabId: string, direction: Direction, sources: string[], targetDir: string): Promise<void>;
  resolve(tabId: string, decision: Decision, applyToAll: boolean): Promise<void>;
  cancel(tabId: string, target: CancelTarget): void;
  retry(tabId: string, jobId: number): void;
  clear(tabId: string): Promise<void>;
  setView(tabId: string, view: TransferView): void;
  toggleCollapsed(tabId: string): void;
  dropTab(tabId: string): void;
}

const sessionOf = (tabId: string) => useSftpStore.getState().tabs.find((t) => t.id === tabId)?.sessionId;

export const useTransfersStore = create<TransfersState>()((set, get) => {
  const patch = (tabId: string, change: (t: TabTransfers) => Partial<TabTransfers>) =>
    set((s) => {
      const t = s.byTab[tabId] ?? empty();
      return { byTab: { ...s.byTab, [tabId]: { ...t, ...change(t) } } };
    });

  return {
    byTab: {},

    onEvent: (tabId, event, now = Date.now()) => {
      switch (event.kind) {
        case "job":
          patch(tabId, (t) => ({
            jobs: { ...t.jobs, [event.job.id]: event.job },
            order: t.order.includes(event.job.id) ? t.order : [...t.order, event.job.id],
            conflicts: event.job.state === "waitingDecision" ? t.conflicts : t.conflicts.filter((c) => c.jobId !== event.job.id),
            samples: event.job.state === "running" ? t.samples : { ...t.samples, [event.job.id]: [] },
          }));
          break;
        case "progress":
          patch(tabId, (t) => {
            const old = t.jobs[event.jobId];
            const samples = [...(t.samples[event.jobId] ?? []), { at: now, done: event.done }].filter((s) => now - s.at <= SPEED_WINDOW_MS);
            return {
              jobs: old ? { ...t.jobs, [event.jobId]: { ...old, done: event.done } } : t.jobs,
              samples: { ...t.samples, [event.jobId]: samples },
            };
          });
          break;
        case "conflict":
          patch(tabId, (t) => ({ conflicts: [...t.conflicts, { jobId: event.jobId, source: event.source, target: event.target }] }));
          break;
        case "batchDone": {
          const side = event.direction === "upload" ? "remote" : "local";
          const tab = useSftpStore.getState().tabs.find((x) => x.id === tabId);
          if (tab?.panes[side].path === event.targetDir) void useSftpStore.getState().reload(tabId, side);
          break;
        }
      }
    },

    enqueue: async (tabId, direction, sources, targetDir) => {
      const sessionId = sessionOf(tabId);
      if (!sessionId) return;
      patch(tabId, () => ({ view: "queue", collapsed: false }));
      await transferEnqueue(sessionId, direction, sources, targetDir);
    },

    resolve: async (tabId, decision, applyToAll) => {
      const sessionId = sessionOf(tabId);
      const first = get().byTab[tabId]?.conflicts[0];
      if (!sessionId || !first) return;
      patch(tabId, (t) => ({ conflicts: t.conflicts.slice(1) }));
      await transferResolve(sessionId, first.jobId, decision, applyToAll);
    },

    cancel: (tabId, target) => {
      const sessionId = sessionOf(tabId);
      if (sessionId) fireAndForget(() => transferCancel(sessionId, target));
    },

    retry: (tabId, jobId) => {
      const sessionId = sessionOf(tabId);
      if (sessionId) fireAndForget(() => transferRetry(sessionId, jobId));
    },

    clear: async (tabId) => {
      const sessionId = sessionOf(tabId);
      if (!sessionId) return;
      patch(tabId, (t) => {
        const keep = t.order.filter((id) => viewOf(t.jobs[id]!.state) !== "done");
        return { order: keep, jobs: Object.fromEntries(keep.map((id) => [id, t.jobs[id]!])) };
      });
      await transferClear(sessionId);
    },

    setView: (tabId, view) => patch(tabId, () => ({ view })),
    toggleCollapsed: (tabId) => patch(tabId, (t) => ({ collapsed: !t.collapsed })),
    dropTab: (tabId) =>
      set((s) => {
        const { [tabId]: _gone, ...rest } = s.byTab;
        return { byTab: rest };
      }),
  };
});
