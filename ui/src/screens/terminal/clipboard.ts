/**
 * Clipboard access for the terminal, through the Tauri clipboard plugin: WebKitGTK's
 * `navigator.clipboard` cannot read what other applications copied. The X11/Wayland PRIMARY
 * selection is not available either, so "select to copy / middle click to paste" uses the
 * regular clipboard.
 */
import { readText as readClipboard, writeText as writeClipboard } from "@tauri-apps/plugin-clipboard-manager";

export async function copyText(text: string): Promise<void> {
  try {
    await writeClipboard(text);
  } catch {
    // clipboard unavailable – selection simply stays local
  }
}

/** Empty when the clipboard holds no text (the plugin rejects then) or cannot be read. */
export async function readText(): Promise<string> {
  try {
    return await readClipboard();
  } catch {
    return "";
  }
}
