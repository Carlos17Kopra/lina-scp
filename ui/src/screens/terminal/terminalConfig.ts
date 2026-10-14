import type { TerminalRenderer } from "../../ipc/api";

/**
 * WebGL only on the visible terminal (WebKit allows ~16 live WebGL contexts) and only until it
 * failed once for this terminal; `dom` never uses WebGL.
 */
export function shouldUseWebgl(setting: TerminalRenderer, visible: boolean, failed: boolean): boolean {
  return visible && setting !== "dom" && !failed;
}

type KeyInfo = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** Ctrl+Shift+C copies the selection, Ctrl+Shift+V pastes (plain Ctrl+C/V belong to the shell). */
export function clipboardShortcut(event: KeyInfo): "copy" | "paste" | null {
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (key === "c") return "copy";
  if (key === "v") return "paste";
  return null;
}
