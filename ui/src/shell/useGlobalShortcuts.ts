import { useEffect } from "react";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";

export type Shortcut = "search" | "newHost" | "newLocalTerminal" | "settings";

type KeyInfo = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** Maps a key event to an app shortcut (Ctrl+K, Ctrl+N, Ctrl+Shift+T, Ctrl+,). */
export function shortcutFor(event: KeyInfo): Shortcut | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (event.shiftKey) return key === "t" ? "newLocalTerminal" : null;
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

/** Shortcuts that also apply while focus is inside a terminal (`[data-terminal]`). */
const TERMINAL_SAFE: ReadonlySet<Shortcut> = new Set<Shortcut>(["newLocalTerminal", "settings"]);

function insideTerminal(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("[data-terminal]") !== null;
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
        case "settings":
          ui.openModal({ type: "settings" });
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
