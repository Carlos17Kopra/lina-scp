import { create } from "zustand";
import type { Id, Theme } from "../ipc/api";

export type View = "hosts" | "terminal" | "sftp";

/** `id` undefined = create a new host/profile. */
export type EditorTarget = { kind: "host"; id?: Id } | { kind: "profile"; id?: Id };

export type ModalState =
  | { type: "editor"; target: EditorTarget }
  | { type: "settings" }
  | { type: "firstRun" }
  | { type: "export" }
  | { type: "import" }
  | { type: "connect" }
  /** `id` undefined = new snippet; `hostId` preselects its scope. */
  | { type: "snippet"; id?: Id; hostId?: Id | null };

export interface UiState {
  view: View;
  setView(view: View): void;
  modal: ModalState | null;
  openModal(modal: ModalState): void;
  closeModal(): void;
  theme: Theme;
  /** Applies the theme immediately; persisting is the settings store's job. */
  setTheme(theme: Theme): void;
  /** Incremented by Ctrl+K; the host search input focuses itself when it changes. */
  searchFocusToken: number;
  focusSearch(): void;
}

/** Tokens live on `.app` (= <html>); `.app.light` switches to the light palette. */
export function applyTheme(theme: Theme): void {
  document.documentElement.classList.toggle("light", theme === "light");
}

export const useUiStore = create<UiState>()((set) => ({
  view: "hosts",
  setView: (view) => set({ view }),
  modal: null,
  openModal: (modal) => set({ modal }),
  closeModal: () => set({ modal: null }),
  theme: "dark",
  setTheme: (theme) => {
    applyTheme(theme);
    set({ theme });
  },
  searchFocusToken: 0,
  focusSearch: () => set((state) => ({ searchFocusToken: state.searchFocusToken + 1 })),
}));
