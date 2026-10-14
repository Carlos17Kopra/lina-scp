import { create } from "zustand";
import { type CoreError, type Settings, settingsGet, settingsSet } from "../ipc/api";
import { t } from "../i18n/de";

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
  /**
   * Optimistic: applies the patch immediately, keeps the persisted result, reverts and rethrows on error.
   * Rejects with a `config` CoreError (without calling the backend) until `load()` succeeded, so the
   * defaults can never overwrite the persisted settings (e.g. switch mirror mode off unconfirmed).
   */
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
    if (!get().loaded) {
      const error: CoreError = { type: "config", message: t.settings.notLoaded };
      throw error;
    }
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
