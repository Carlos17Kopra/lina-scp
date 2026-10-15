import { create } from "zustand";
import { type Id, type Snippet, snippetDelete, snippetSave, snippetsList } from "../ipc/api";

export interface SnippetsState {
  /** In core order: by name. */
  snippets: Snippet[];
  loaded: boolean;
  load(): Promise<void>;
  save(snippet: Snippet): Promise<Snippet>;
  remove(id: Id): Promise<void>;
}

export const useSnippetsStore = create<SnippetsState>()((set, get) => ({
  snippets: [],
  loaded: false,
  load: async () => {
    set({ snippets: await snippetsList(), loaded: true });
  },
  save: async (snippet) => {
    const saved = await snippetSave(snippet);
    await get().load();
    return saved;
  },
  remove: async (id) => {
    await snippetDelete(id);
    await get().load();
  },
}));
