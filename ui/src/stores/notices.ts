import { create } from "zustand";
import type { Tone } from "../components";

export interface Notice {
  id: number;
  tone: Tone;
  message: string;
  details?: string;
}

export interface NoticeState {
  notices: Notice[];
  push(notice: Omit<Notice, "id">): void;
  remove(id: number): void;
}

let nextNoticeId = 1;

/** Messages from stores (which cannot use the toast context); `useNoticeToasts` shows them. */
export const useNoticeStore = create<NoticeState>()((set) => ({
  notices: [],
  push: (notice) => {
    const id = nextNoticeId;
    nextNoticeId += 1;
    set((state) => ({ notices: [...state.notices, { ...notice, id }] }));
  },
  remove: (id) => set((state) => ({ notices: state.notices.filter((notice) => notice.id !== id) })),
}));

export function pushNotice(notice: Omit<Notice, "id">): void {
  useNoticeStore.getState().push(notice);
}
