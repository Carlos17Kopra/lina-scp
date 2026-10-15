/** What the snippets panel can do with a tab's terminal. `TerminalView` registers it for its lifetime. */
export interface PasteTarget {
  /** Pastes like Ctrl+Shift+V (bracketed paste if the shell enabled it) and focuses the terminal. */
  insert(text: string): void;
  /** `insert`, then Enter. */
  run(text: string): void;
}

const targets = new Map<string, PasteTarget>();

export function registerPasteTarget(tabId: string, target: PasteTarget): void {
  targets.set(tabId, target);
}

export function dropPasteTarget(tabId: string): void {
  targets.delete(tabId);
}

export function getPasteTarget(tabId: string): PasteTarget | undefined {
  return targets.get(tabId);
}
