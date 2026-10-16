import { create } from "zustand";
import { t } from "../i18n/de";
import { type EditEvent, type EditFile, type OpenApp, editOpen, editReopen, editResolve, editStop, localEdit } from "../ipc/api";
import { errorDetails } from "../ipc/errors";
import { pushNotice } from "./notices";
import { type Side, useSftpStore } from "./sftp";
import { useTransfersStore } from "./transfers";

export interface EditsState {
  /** Per SFTP tab, in arrival order. */
  byTab: Record<string, EditFile[]>;
  onEvent(tabId: string, event: EditEvent): void;
  /** Remote: download, watch and open; local: open in place. Failures become notices. */
  open(tabId: string, side: Side, path: string, how: OpenApp): Promise<void>;
  resolve(tabId: string, id: number, overwrite: boolean): Promise<void>;
  reopen(tabId: string, id: number): Promise<void>;
  stop(tabId: string, id: number): Promise<void>;
  drop(tabId: string): void;
}

export const dirtyCount = (files: EditFile[] | undefined) => (files ?? []).filter((f) => f.dirty).length;
export const conflictOf = (files: EditFile[] | undefined) => files?.find((f) => f.state === "conflict");

const sessionOf = (tabId: string) => useSftpStore.getState().tabs.find((x) => x.id === tabId)?.sessionId;

export function noticeKept(path: string) {
  pushNotice({ tone: "amber", message: t.sftp.edit.kept(path) });
}

function noticeFailed(error: unknown) {
  const noEditor = typeof error === "object" && error !== null && "type" in error && error.type === "config";
  pushNotice({ tone: "red", message: noEditor ? t.sftp.edit.noEditor : t.sftp.edit.failed, details: errorDetails(error) });
}

export const useEditsStore = create<EditsState>()((set) => {
  const patch = (tabId: string, id: number, change: Partial<EditFile>) =>
    set((s) => ({ byTab: { ...s.byTab, [tabId]: (s.byTab[tabId] ?? []).map((f) => (f.id === id ? { ...f, ...change } : f)) } }));

  return {
    byTab: {},

    onEvent: (tabId, event) =>
      set((s) => {
        const files = s.byTab[tabId] ?? [];
        const { file } = event;
        const next = files.some((f) => f.id === file.id) ? files.map((f) => (f.id === file.id ? file : f)) : [...files, file];
        return { byTab: { ...s.byTab, [tabId]: next } };
      }),

    open: async (tabId, side, path, how) => {
      try {
        if (side === "local") {
          await localEdit(path, how);
          return;
        }
        const sessionId = sessionOf(tabId);
        if (!sessionId) return;
        const transfers = useTransfersStore.getState();
        transfers.setView(tabId, "edits");
        if (transfers.byTab[tabId]?.collapsed) transfers.toggleCollapsed(tabId);
        await editOpen(sessionId, path, how);
      } catch (error) {
        noticeFailed(error);
      }
    },

    resolve: async (tabId, id, overwrite) => {
      const sessionId = sessionOf(tabId);
      if (!sessionId) return;
      patch(tabId, id, { state: overwrite ? "uploading" : "changed" });
      try {
        await editResolve(sessionId, id, overwrite);
      } catch {
        // The next event carries the real state.
      }
    },

    reopen: async (_tabId, id) => {
      const sessionId = sessionOf(_tabId);
      if (!sessionId) return;
      try {
        await editReopen(sessionId, id);
      } catch (error) {
        noticeFailed(error);
      }
    },

    stop: async (tabId, id) => {
      const sessionId = sessionOf(tabId);
      if (!sessionId) return;
      try {
        const kept = await editStop(sessionId, id);
        set((s) => ({ byTab: { ...s.byTab, [tabId]: (s.byTab[tabId] ?? []).filter((f) => f.id !== id) } }));
        if (kept) noticeKept(kept);
      } catch (error) {
        noticeFailed(error);
      }
    },

    drop: (tabId) =>
      set((s) => {
        const { [tabId]: _gone, ...rest } = s.byTab;
        return { byTab: rest };
      }),
  };
});
