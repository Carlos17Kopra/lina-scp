import type { PaneState } from "../../stores/sftp";

export function moveCursor(names: string[], cursor: string | null, key: string, page: number): string | null {
  if (names.length === 0) return null;
  const last = names.length - 1;
  const at = cursor === null ? -1 : names.indexOf(cursor);
  const index = (() => {
    switch (key) {
      case "ArrowDown":
        return Math.min(at + 1, last);
      case "ArrowUp":
        return Math.max(at - 1, 0);
      case "Home":
        return 0;
      case "End":
        return last;
      case "PageDown":
        return Math.min(Math.max(at, 0) + page, last);
      case "PageUp":
        return Math.max(at - page, 0);
      default:
        return at;
    }
  })();
  return names[index] ?? null;
}

export function rangeBetween(names: string[], a: string, b: string): string[] {
  const [i, j] = [names.indexOf(a), names.indexOf(b)].sort((x, y) => x - y) as [number, number];
  return names.slice(i, j + 1).filter((n) => n !== "..");
}

export function typeAhead(names: string[], prefix: string, from: string | null): string | null {
  const p = prefix.toLocaleLowerCase("de");
  const start = from === null ? 0 : names.indexOf(from);
  // A continued prefix may stay on the current entry; a fresh letter moves on.
  const order = [...names.slice(start), ...names.slice(0, start)];
  const firstMatch = (list: string[]) => list.find((n) => n !== ".." && n.toLocaleLowerCase("de").startsWith(p)) ?? null;
  return prefix.length > 1 ? firstMatch(order) : firstMatch([...order.slice(1), ...order.slice(0, 1)]);
}

/** Entries an action applies to: the selection, else the cursor; never `..` or lossy names. */
export function targets(pane: PaneState): string[] {
  const usable = (name: string) => name !== ".." && pane.entries.some((e) => e.name === name && !e.nameLossy);
  const chosen = pane.selection.length > 0 ? pane.selection : pane.cursor ? [pane.cursor] : [];
  return chosen.filter(usable);
}
