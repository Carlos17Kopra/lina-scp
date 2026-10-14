import { create } from "zustand";
import { type Settings, settingsGet, settingsSet } from "../ipc/api";

export const DEFAULT_SETTINGS: Settings = {
  theme: "dark",
  sshMode: "integrate",
  terminalRenderer: "auto",
  terminalFontSize: 14,
  firstRunDone: false,
  compatRendering: false,
};

export interface SettingsState {
  settings: Settings;
  loaded: boolean;
  load(): Promise<void>;
  /** Optimistic: applies the patch immediately, keeps the persisted result, reverts and rethrows on error. */
  save(patch: Partial<Settings>): Promise<Settings>;
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  settings: DEFAULT_SETTINGS,
  loaded: false,
  load: async () => {
    const settings = await settingsGet();
    set({ settings, loaded: true });
  },
  save: async (patch) => {
    const previous = get().settings;
    const next = { ...previous, ...patch };
    set({ settings: next });
    try {
      const saved = await settingsSet(next);
      set({ settings: saved });
      return saved;
    } catch (error) {
      set({ settings: previous });
      throw error;
    }
  },
}));
