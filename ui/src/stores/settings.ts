import { create } from "zustand";
import { type CoreError, type Settings, settingsGet, settingsSet } from "../ipc/api";
import { t } from "../i18n/de";

export const DEFAULT_SETTINGS: Settings = {
  theme: "dark",
  sshMode: "integrate",
  terminalRenderer: "auto",
  terminalFontSize: 14,
  terminalScheme: "auto",
  terminalSidebarOpen: false,
  autoReconnect: true,
  firstRunDone: false,
  compatRendering: false,
  editorCommand: null,
  terminalScrollback: 10000,
  terminalCursor: "block",
  terminalCursorBlink: true,
  localShell: null,
  newHostUser: null,
  newHostPort: 22,
  newHostKeepaliveS: 30,
  newHostTimeoutS: 15,
};

export interface SettingsState {
  settings: Settings;
  loaded: boolean;
  load(): Promise<void>;
  /**
   * Optimistic: applies the patch immediately and rethrows on error. Saves run one after another, each sends the
   * last saved state plus its own patch, so a failed one undoes only its own fields and a slow one never
   * overwrites a later one. Rejects with a `config` CoreError (without calling the backend) until `load()`
   * succeeded, so the defaults can never overwrite the persisted settings (e.g. switch mirror mode off unconfirmed).
   */
  save(patch: Partial<Settings>): Promise<Settings>;
  /** Internal: the save chain, its unfinished patches, and the state they apply on. */
  queue: Promise<unknown>;
  pending: Array<Partial<Settings>>;
  saved: Settings;
}

export const useSettingsStore = create<SettingsState>()((set, get) => {
  const shown = () => get().pending.reduce<Settings>((s, p) => ({ ...s, ...p }), get().saved);
  return {
    settings: DEFAULT_SETTINGS,
    loaded: false,
    queue: Promise.resolve(),
    pending: [],
    saved: DEFAULT_SETTINGS,
    load: async () => {
      const settings = await settingsGet();
      set({ settings, saved: settings, loaded: true });
    },
    save: (patch) => {
      if (!get().loaded) {
        const error: CoreError = { type: "config", message: t.settings.notLoaded };
        return Promise.reject(error);
      }
      // Nothing in flight: what is shown (loaded, or set from outside) is the saved state.
      if (get().pending.length === 0) set({ saved: get().settings });
      set({ pending: [...get().pending, patch] });
      set({ settings: shown() });
      const step = get().queue.then(async () => {
        try {
          const saved = await settingsSet({ ...get().saved, ...patch });
          set({ saved });
          return saved;
        } finally {
          set({ pending: get().pending.filter((p) => p !== patch) });
          set({ settings: shown() });
        }
      });
      set({ queue: step.catch(() => undefined) });
      return step;
    },
  };
});
