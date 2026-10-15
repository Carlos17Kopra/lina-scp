import { useEffect } from "react";
import { toggleTerminalSidebar } from "../screens/terminal/sidebar/toggle";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";

export type Shortcut =
  | "search"
  | "newHost"
  | "newLocalTerminal"
  | "toggleSidebar"
  | "settings"
  | "splitRow"
  | "splitColumn"
  | "toggleBroadcast"
  | "focusPrev"
  | "focusNext"
  | "closeTab";

const SHIFT_SHORTCUTS: Readonly<Record<string, Shortcut>> = {
  t: "newLocalTerminal",
  b: "toggleSidebar",
  e: "splitRow",
  o: "splitColumn",
  a: "toggleBroadcast",
  w: "closeTab",
  arrowleft: "focusPrev",
  arrowright: "focusNext",
};

type KeyInfo = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/**
 * Maps a key event to an app shortcut: Ctrl+K, Ctrl+N, Ctrl+, and Ctrl+Shift+T/B/E/O/A/W/←/→.
 */
export function shortcutFor(event: KeyInfo): Shortcut | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (event.shiftKey) return SHIFT_SHORTCUTS[key] ?? null;
  switch (key) {
    case "k":
      return "search";
    case "n":
      return "newHost";
    case ",":
      return "settings";
    default:
      return null;
  }
}

/** Shortcuts that belong to the terminal view: they work only there, also with focus inside a terminal. */
const TERMINAL_VIEW_ONLY: ReadonlySet<Shortcut> = new Set<Shortcut>([
  "toggleSidebar",
  "splitRow",
  "splitColumn",
  "toggleBroadcast",
  "focusPrev",
  "focusNext",
  "closeTab",
]);
/** Shortcuts that also apply while focus is inside a terminal (`[data-terminal]`). */
const TERMINAL_SAFE: ReadonlySet<Shortcut> = new Set<Shortcut>(["newLocalTerminal", "settings", ...TERMINAL_VIEW_ONLY]);

function insideTerminal(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("[data-terminal]") !== null;
}

/** Ctrl+Shift+←/→ selects words in text fields; there the pane keys stay with the field. */
const WORD_SELECTION: ReadonlySet<Shortcut> = new Set<Shortcut>(["focusPrev", "focusNext"]);

function inTextField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement) || insideTerminal(target)) return false;
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target.isContentEditable;
}

/**
 * Registers the global shortcuts on `window` in the capture phase, so they win over xterm.js
 * (which stops propagation of keys it handles). Inactive while a modal is open.
 */
export function useGlobalShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const shortcut = shortcutFor(event);
      if (!shortcut) return;
      const ui = useUiStore.getState();
      if (ui.modal) return;
      if (!TERMINAL_SAFE.has(shortcut) && insideTerminal(event.target)) return;
      if (WORD_SELECTION.has(shortcut) && inTextField(event.target)) return;
      // Sidebar, panes and tabs belong to the terminal view; elsewhere the keys stay unused.
      if (TERMINAL_VIEW_ONLY.has(shortcut) && ui.view !== "terminal") return;
      event.preventDefault();
      event.stopPropagation();
      switch (shortcut) {
        case "search":
          ui.setView("hosts");
          ui.focusSearch();
          break;
        case "newHost":
          ui.openModal({ type: "editor", target: { kind: "host" } });
          break;
        case "newLocalTerminal":
          void useSessionsStore.getState().openLocal();
          break;
        case "toggleSidebar":
          toggleTerminalSidebar();
          break;
        case "settings":
          ui.openModal({ type: "settings" });
          break;
        case "splitRow":
          useSessionsStore.getState().split("row");
          break;
        case "splitColumn":
          useSessionsStore.getState().split("column");
          break;
        case "toggleBroadcast":
          useSessionsStore.getState().toggleBroadcast();
          break;
        case "focusPrev":
          useSessionsStore.getState().focusNeighbour(-1);
          break;
        case "focusNext":
          useSessionsStore.getState().focusNeighbour(1);
          break;
        case "closeTab": {
          const { activeId, closeGroup } = useSessionsStore.getState();
          if (activeId) void closeGroup(activeId);
          break;
        }
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
