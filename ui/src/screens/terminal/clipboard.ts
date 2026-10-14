/**
 * Clipboard access for the terminal. WebKitGTK does not expose the X11/Wayland PRIMARY selection to
 * web content, so "select to copy / middle click to paste" uses the regular clipboard.
 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard?.writeText(text);
  } catch {
    // clipboard unavailable (permissions) – selection simply stays local
  }
}

export async function readText(): Promise<string> {
  try {
    return (await navigator.clipboard?.readText()) ?? "";
  } catch {
    return "";
  }
}
