/** Split-pane layout of one terminal tab: a binary tree whose leaves are pane (= session tab) ids. */
export type SplitDir = "row" | "column";
export type Layout =
  | { type: "pane"; paneId: string }
  | { type: "split"; dir: SplitDir; ratio: number; a: Layout; b: Layout };
/** Steps from the root to a split ("a" = first child). */
export type SplitPath = ReadonlyArray<"a" | "b">;
/** Fractions (0–1) of the tab's pane area. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Arrangement {
  panes: Array<{ paneId: string; rect: Rect }>;
  dividers: Array<{ path: SplitPath; dir: SplitDir; ratio: number; area: Rect }>;
}

export const MAX_PANES = 4;
export const MIN_RATIO = 0.15;
export const MAX_RATIO = 0.85;
const FULL: Rect = { x: 0, y: 0, w: 1, h: 1 };

export const leaf = (paneId: string): Layout => ({ type: "pane", paneId });

export function paneIds(layout: Layout): string[] {
  return layout.type === "pane" ? [layout.paneId] : [...paneIds(layout.a), ...paneIds(layout.b)];
}

/** Splits `paneId`; the new pane goes right (row) or below (column) at 50 %. */
export function splitPane(layout: Layout, paneId: string, dir: SplitDir, newPaneId: string): Layout {
  if (layout.type === "pane") {
    return layout.paneId === paneId ? { type: "split", dir, ratio: 0.5, a: layout, b: leaf(newPaneId) } : layout;
  }
  return { ...layout, a: splitPane(layout.a, paneId, dir, newPaneId), b: splitPane(layout.b, paneId, dir, newPaneId) };
}

/** Removes `paneId`; its sibling takes the split's place. `null` when the last pane goes. */
export function removePane(layout: Layout, paneId: string): Layout | null {
  if (layout.type === "pane") return layout.paneId === paneId ? null : layout;
  const a = removePane(layout.a, paneId);
  const b = removePane(layout.b, paneId);
  if (!a) return b;
  if (!b) return a;
  return a === layout.a && b === layout.b ? layout : { ...layout, a, b };
}

export function renamePane(layout: Layout, oldId: string, newId: string): Layout {
  if (layout.type === "pane") return layout.paneId === oldId ? leaf(newId) : layout;
  return { ...layout, a: renamePane(layout.a, oldId, newId), b: renamePane(layout.b, oldId, newId) };
}

export function clampRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(MAX_RATIO, Math.max(MIN_RATIO, ratio));
}

export function setRatio(layout: Layout, path: SplitPath, ratio: number): Layout {
  if (layout.type === "pane") return layout;
  const [head, ...rest] = path;
  if (head === undefined) return { ...layout, ratio: clampRatio(ratio) };
  return head === "a" ? { ...layout, a: setRatio(layout.a, rest, ratio) } : { ...layout, b: setRatio(layout.b, rest, ratio) };
}

/** Next (`1`) / previous (`-1`) pane in tree order, cyclic; the first pane for an unknown id. */
export function neighbour(layout: Layout, paneId: string, step: 1 | -1): string {
  const ids = paneIds(layout);
  const index = ids.indexOf(paneId);
  if (index === -1) return ids[0] as string;
  return ids[(index + step + ids.length) % ids.length] as string;
}

/** Pane rectangles and divider positions, so panes can render flat (no remount when the tree changes). */
export function arrange(layout: Layout, area: Rect = FULL, path: SplitPath = []): Arrangement {
  if (layout.type === "pane") return { panes: [{ paneId: layout.paneId, rect: area }], dividers: [] };
  const { ratio } = layout;
  const [first, second]: [Rect, Rect] =
    layout.dir === "row"
      ? [
          { ...area, w: area.w * ratio },
          { ...area, x: area.x + area.w * ratio, w: area.w * (1 - ratio) },
        ]
      : [
          { ...area, h: area.h * ratio },
          { ...area, y: area.y + area.h * ratio, h: area.h * (1 - ratio) },
        ];
  const a = arrange(layout.a, first, [...path, "a"]);
  const b = arrange(layout.b, second, [...path, "b"]);
  return {
    panes: [...a.panes, ...b.panes],
    dividers: [{ path, dir: layout.dir, ratio, area }, ...a.dividers, ...b.dividers],
  };
}
