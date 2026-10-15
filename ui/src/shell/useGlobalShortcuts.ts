import { useEffect } from "react";
import { toggleTerminalSidebar } from "../screens/terminal/sidebar/toggle";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";

export type Shortcut = "search" | "newHost" | "newLocalTerminal" | "toggleSidebar" | "settings";

type KeyInfo = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** Maps a key event to an app shortcut (Ctrl+K, Ctrl+N, Ctrl+Shift+T, Ctrl+Shift+B, Ctrl+,). */
export function shortcutFor(event: KeyInfo): Shortcut | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (event.shiftKey) {
    if (key === "t") return "newLocalTerminal";
    if (key === "b") return "toggleSidebar";
    return null;
  }
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
const TERMINAL_SAFE: ReadonlySet<Shortcut> = new Set<Shortcut>(["newLocalTerminal", "toggleSidebar", "settings"]);

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
      // The sidebar belongs to the terminal view; elsewhere the key stays unused.
      if (shortcut === "toggleSidebar" && ui.view !== "terminal") return;
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
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
