import { create } from "zustand";
import { t } from "../i18n/de";
import { type Bookmark, bookmarkAdd, bookmarkRemove, bookmarksList, type Id } from "../ipc/api";
import { errorDetails } from "../ipc/errors";
import { pushNotice } from "./notices";

/** `"local"` for the local pane, else the host id. */
export type BookmarkKey = "local" | Id;

export interface BookmarksState {
  byKey: Record<string, Bookmark[]>;
  load(key: BookmarkKey): Promise<void>;
  /** Adds the bookmark of `path`, or removes it when it exists. */
  toggle(key: BookmarkKey, path: string): Promise<void>;
  remove(key: BookmarkKey, id: Id): Promise<void>;
}

const hostOf = (key: BookmarkKey) => (key === "local" ? null : key);

/** Runs `op`, then reloads `key`; failures become a notice (callers never see a rejection). */
async function guarded(key: BookmarkKey, op: () => Promise<unknown>) {
  try {
    await op();
    await useBookmarksStore.getState().load(key);
  } catch (error) {
    pushNotice({ tone: "red", message: t.sftp.bookmarks.failed, details: errorDetails(error) });
  }
}

export const useBookmarksStore = create<BookmarksState>()((set, get) => ({
  byKey: {},
  load: async (key) => {
    const list = await bookmarksList(hostOf(key));
    set((s) => ({ byKey: { ...s.byKey, [key]: list } }));
  },
  toggle: (key, path) => {
    const existing = get().byKey[key]?.find((b) => b.path === path);
    return guarded(key, () => (existing ? bookmarkRemove(existing.id) : bookmarkAdd(hostOf(key), path)));
  },
  remove: (key, id) => guarded(key, () => bookmarkRemove(id)),
}));
