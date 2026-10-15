# LinaSCP TP2c (Split Panes + Broadcast) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A terminal tab can be split into up to 4 nested panes, each with its own session, and broadcast sends the
focused pane's input to every connected pane of that tab.

**Architecture:**
- **Pane = session.** The existing `Tab` record (one session) becomes a pane. New `TabGroup`s (`layout` tree,
  `focusedPaneId`, `broadcast`) sit on top and form the UI tabs.
- **Layout logic:** lives in pure functions (`layout.ts`).
- **Rendering:** panes are rendered **flat**, keyed by pane id and absolutely positioned from `arrange(layout)`, so
  splitting never remounts a running xterm.
- **New pane:** shows a picker first. SSH goes through the normal connect flow with a `paneId` target, a local shell
  through `openLocal({ paneId })`.
- **Broadcast:** happens in `TerminalView`'s input path.

**Tech Stack:** React 19, zustand, xterm.js 6, Vitest + Testing Library, bun. No backend changes.

**Spec:** `docs/superpowers/specs/2026-10-15-linascp-tp2c-split-broadcast-design.md`

## Global Constraints

- **Branch and commits:** work on `feat/tp2c-split-broadcast` (exists, holds the spec). Commits end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Language:** code and identifiers are in English. UI strings are German and live only in `ui/src/i18n/de.ts`.
- **Panes:**
  - At most 4 panes per tab (`MAX_PANES = 4`).
  - The split ratio is the share of `a`, clamped to 0.15–0.85. A new split starts at 0.5.
  - `dir: "row"` means side by side, `"column"` means one above the other.
- **Shortcuts** (terminal view only, also while focus is in a terminal, inactive while a modal is open):
  - `Ctrl+Shift+E` splits side by side; `Ctrl+Shift+O` splits one above the other.
  - `Ctrl+Shift+A` toggles broadcast.
  - `Ctrl+Shift+←/→` focuses the previous / next pane.
  - `Ctrl+Shift+W` closes the active tab.
- **Broadcast:**
  - It is a flag per tab and can be switched on only with ≥ 2 panes. It turns off automatically when one pane is left.
  - It sends `onData`/`onBinary` of the **focused** pane to all other panes of the group that are `"connected"` and
    have a `sessionId`. Resize is never broadcast.
- **Layouts are not persisted.**
- **Test output stays free of warnings**, including act() warnings.

## Review Focus

1. **Splitting while a session runs must not remount its xterm.** Scrollback and the running program stay. Panes
   render flat and keyed. Test in Task 5: a split keeps the same `TerminalView` instance.
2. **A late `exit` of a replaced session must not end the reconnected one.**
   - "Neu verbinden" gives the pane a new id.
   - Events for a `"picking"` pane are buffered and applied on attach.
   - Tests in Tasks 2 and 3.
3. **A pane closed while its connection is still running:** the new session is closed and no stray tab appears.
   Test in Task 3.
4. **Broadcast must skip panes that are picking, ended or unfocused**, and stop once only one pane is left. Tests in
   Tasks 2 and 4.
5. **Cancelling a pane connection returns to the terminal view, not to the host overview.** The pane shows the
   picker again. Test in Task 3.

---

### Task 1: Pure layout functions

**Files:**
- Create: `ui/src/screens/terminal/layout.ts`, `ui/src/screens/terminal/layout.test.ts`

**Interfaces:**
- Produces:
  - Types:
    - `SplitDir = "row" | "column"`.
    - `Layout = { type: "pane"; paneId: string } | { type: "split"; dir: SplitDir; ratio: number; a: Layout; b: Layout }`.
    - `SplitPath = ReadonlyArray<"a" | "b">`.
    - `Rect = { x; y; w; h }`, as fractions from 0 to 1.
    - `Arrangement = { panes: Array<{ paneId: string; rect: Rect }>; dividers: Array<{ path: SplitPath; dir: SplitDir; ratio: number; area: Rect }> }`.
  - Constants: `MAX_PANES = 4`, `MIN_RATIO = 0.15`, `MAX_RATIO = 0.85`.
  - Functions:
    - `leaf(paneId)`, `paneIds(layout)`.
    - `splitPane(layout, paneId, dir, newPaneId)`, `removePane(layout, paneId): Layout | null`.
    - `renamePane(layout, oldId, newId)`.
    - `clampRatio(r)`, `setRatio(layout, path, ratio)`.
    - `neighbour(layout, paneId, step: 1 | -1)`.
    - `arrange(layout)`.

- [ ] **Step 1: Write the failing tests**

`ui/src/screens/terminal/layout.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  arrange,
  clampRatio,
  type Layout,
  leaf,
  neighbour,
  paneIds,
  removePane,
  renamePane,
  setRatio,
  splitPane,
} from "./layout";

// a | (b / c)
const three: Layout = {
  type: "split",
  dir: "row",
  ratio: 0.5,
  a: leaf("a"),
  b: { type: "split", dir: "column", ratio: 0.5, a: leaf("b"), b: leaf("c") },
};

describe("layout", () => {
  it("splits a pane: the new one goes right / below at 50 %", () => {
    expect(splitPane(leaf("a"), "a", "row", "b")).toEqual({ type: "split", dir: "row", ratio: 0.5, a: leaf("a"), b: leaf("b") });
    expect(paneIds(splitPane(three, "b", "row", "d"))).toEqual(["a", "b", "d", "c"]);
    expect(splitPane(three, "zz", "row", "d")).toEqual(three);
  });

  it("removes a pane: the sibling subtree takes the split's place; the last pane yields null", () => {
    expect(removePane(three, "a")).toEqual(three.type === "split" ? three.b : null);
    expect(removePane(three, "c")).toEqual({ type: "split", dir: "row", ratio: 0.5, a: leaf("a"), b: leaf("b") });
    expect(removePane(leaf("a"), "a")).toBeNull();
    expect(removePane(three, "zz")).toBe(three);
  });

  it("renames a pane in place", () => {
    expect(paneIds(renamePane(three, "b", "n"))).toEqual(["a", "n", "c"]);
  });

  it("walks panes cyclically in tree order", () => {
    expect(paneIds(three)).toEqual(["a", "b", "c"]);
    expect(neighbour(three, "c", 1)).toBe("a");
    expect(neighbour(three, "a", -1)).toBe("c");
    expect(neighbour(three, "b", 1)).toBe("c");
    expect(neighbour(three, "zz", 1)).toBe("a");
  });

  it("clamps ratios, also for nested paths and garbage input", () => {
    expect(clampRatio(0.01)).toBe(0.15);
    expect(clampRatio(2)).toBe(0.85);
    expect(clampRatio(Number.NaN)).toBe(0.5);
    const moved = setRatio(three, ["b"], 0.7);
    expect(moved.type === "split" && moved.b.type === "split" && moved.b.ratio).toBe(0.7);
    expect(setRatio(three, [], 0.99).type === "split" && (setRatio(three, [], 0.99) as { ratio: number }).ratio).toBe(0.85);
  });

  it("arranges panes and dividers as fractions of the area", () => {
    const { panes, dividers } = arrange(three);
    expect(panes).toEqual([
      { paneId: "a", rect: { x: 0, y: 0, w: 0.5, h: 1 } },
      { paneId: "b", rect: { x: 0.5, y: 0, w: 0.5, h: 0.5 } },
      { paneId: "c", rect: { x: 0.5, y: 0.5, w: 0.5, h: 0.5 } },
    ]);
    expect(dividers).toEqual([
      { path: [], dir: "row", ratio: 0.5, area: { x: 0, y: 0, w: 1, h: 1 } },
      { path: ["b"], dir: "column", ratio: 0.5, area: { x: 0.5, y: 0, w: 0.5, h: 1 } },
    ]);
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `bun run test ui/src/screens/terminal/layout.test.ts 2>&1 | tail -5`
Expected: FAIL, because `./layout` cannot be resolved.

- [ ] **Step 3: Implement `ui/src/screens/terminal/layout.ts`**

```ts
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
```

- [ ] **Step 4: Run the tests**

Run: `bun run test ui/src/screens/terminal/layout.test.ts 2>&1 | tail -5`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/terminal/layout.ts ui/src/screens/terminal/layout.test.ts
git commit -m "feat(ui): pure split-pane layout functions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Sessions store with tab groups

**Files:**
- Modify: `ui/src/stores/sessions.ts`, `ui/src/stores/sessionEvents.ts`, `ui/src/test/fixtures.ts`,
  `ui/src/i18n/de.ts`
- Test: `ui/src/stores/sessions.test.ts`

**Interfaces:**
- Consumes: Task 1 `Layout`, `SplitDir`, `SplitPath`, `leaf`, `paneIds`, `splitPane`, `removePane`, `renamePane`,
  `setRatio`, `neighbour` and `MAX_PANES`.
- Produces:
  - Status and pane fields:
    - `TabStatus` gains `"picking"`.
    - `Tab.origin?: { kind: TabKind; hostId?: Id }`, set only on picking panes.
  - Group type and state: `TabGroup { id; layout; focusedPaneId; broadcast }` and `SessionsState.groups`. `activeId`
    is a **group** id; a new group's id is its first pane's id.
  - Pane and group actions:
    - `addTab(tab)` creates a single-pane group and activates it.
    - `closeTab(paneId)` removes the pane from its group; the group goes with its last pane.
    - `setActive(groupId)`.
    - `closeSession(paneId)` closes the session and removes the pane.
    - `closeGroup(groupId): Promise<void>`.
    - `replacePane(paneId): string` swaps in a new picking pane with a fresh id, closes the old session and returns
      the new id.
  - Layout and focus actions:
    - `split(dir: SplitDir)`.
    - `focusPane(paneId)`.
    - `focusNeighbour(step: 1 | -1)`.
    - `setRatio(groupId, path, ratio)`.
    - `toggleBroadcast()`.
  - Session plumbing:
    - `attachSession(paneId, patch): boolean` works on picking panes only and applies a buffered session event.
    - `openLocal(target?: { paneId: string })`.
  - Helpers:
    - `activeGroup(state)`.
    - `focusedPane(state)`.
    - `broadcastPeers(paneId): SessionId[]`.
  - `t.terminal.panes` strings.
  - The test fixture `singleGroups(tabs: Tab[]): TabGroup[]`.

- [ ] **Step 1: Add strings**

In `ui/src/i18n/de.ts`, inside `terminal: { … }`, add after `closeTab`:

```ts
    panes: {
      newPane: "Neuer Bereich",
      splitRow: "Nebeneinander teilen",
      splitRowTitle: "Nebeneinander teilen (Ctrl+Shift+E)",
      splitColumn: "Untereinander teilen",
      splitColumnTitle: "Untereinander teilen (Ctrl+Shift+O)",
      broadcast: "Broadcast",
      broadcastTitle: "Eingabe an alle Bereiche dieses Tabs senden (Ctrl+Shift+A)",
      broadcastOn: (count: number) => `Broadcast an · ${count} Bereiche`,
      count: (title: string, count: number) => `${title} · ${count}`,
      close: "Bereich schließen",
      choose: "Was soll in diesem Bereich starten?",
      sameHost: (name: string) => `Gleicher Host (${name})`,
      local: "Lokales Terminal",
      search: "Host suchen",
      choices: "Auswahl",
      noTerminal: "Kein Terminal",
      separator: "Bereichsgröße",
    },
```

- [ ] **Step 2: Write the failing store tests**

Append to `ui/src/stores/sessions.test.ts` (it already mocks `../ipc/api`, numbers `newId` as `t-1, t-2 …` and
resets the store). Add the imports `paneIds` from `../screens/terminal/layout`; `broadcastPeers`, `focusedPane`,
`type Tab` and `type TabStatus` from `./sessions`; `discardEarlySessionEvent` from `./sessionEvents`:

```ts
const pane = (id: string, status: TabStatus = "connected"): Tab => ({
  id,
  title: id,
  kind: "ssh",
  hostId: `h-${id}`,
  sessionId: `s-${id}`,
  status,
});

describe("sessions store — groups and panes", () => {
  it("creates one group per added tab, its id being the pane id", () => {
    const s = useSessionsStore.getState();
    s.addTab(pane("a"));
    s.addTab(pane("b"));
    expect(useSessionsStore.getState().groups).toEqual([
      { id: "a", layout: { type: "pane", paneId: "a" }, focusedPaneId: "a", broadcast: false },
      { id: "b", layout: { type: "pane", paneId: "b" }, focusedPaneId: "b", broadcast: false },
    ]);
    expect(useSessionsStore.getState().activeId).toBe("b");
  });

  it("splits the focused pane into a focused picking pane that remembers its origin, up to 4 panes", () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    const state = useSessionsStore.getState();
    expect(paneIds(state.groups[0]!.layout)).toEqual(["a", "t-1"]);
    expect(state.groups[0]!.focusedPaneId).toBe("t-1");
    expect(focusedPane(state)).toMatchObject({ id: "t-1", status: "picking", origin: { kind: "ssh", hostId: "h-a" } });
    useSessionsStore.getState().split("column");
    useSessionsStore.getState().split("column");
    useSessionsStore.getState().split("row");
    expect(paneIds(useSessionsStore.getState().groups[0]!.layout)).toHaveLength(4);
  });

  it("closes a pane: its session ends, focus moves to the previous pane, the group goes with its last pane", async () => {
    const s = useSessionsStore.getState();
    s.addTab(pane("x"));
    s.addTab(pane("a"));
    s.split("row");
    useSessionsStore.getState().attachSession("t-1", { sessionId: "s-new", status: "connected" });
    await useSessionsStore.getState().closeSession("t-1");
    expect(api.sessionClose).toHaveBeenCalledWith("s-new");
    expect(useSessionsStore.getState().groups[1]).toMatchObject({ layout: { type: "pane", paneId: "a" }, focusedPaneId: "a" });
    await useSessionsStore.getState().closeSession("a");
    expect(useSessionsStore.getState()).toMatchObject({ activeId: "x", groups: [{ id: "x" }] });
  });

  it("closes a whole group with all its sessions", async () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().attachSession("t-1", { sessionId: "s-2", status: "connected" });
    await useSessionsStore.getState().closeGroup("a");
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
    expect(api.sessionClose).toHaveBeenCalledWith("s-2");
    expect(useSessionsStore.getState()).toMatchObject({ tabs: [], groups: [], activeId: null });
  });

  it("moves focus cyclically and sets ratios within bounds", () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().focusNeighbour(1);
    expect(useSessionsStore.getState().groups[0]!.focusedPaneId).toBe("a");
    useSessionsStore.getState().focusPane("t-1");
    expect(useSessionsStore.getState().groups[0]!.focusedPaneId).toBe("t-1");
    useSessionsStore.getState().setRatio("a", [], 0.99);
    expect(useSessionsStore.getState().groups[0]!.layout).toMatchObject({ ratio: 0.85 });
  });

  it("broadcasts only from the focused pane to connected peers and switches off at one pane", async () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().toggleBroadcast();
    expect(useSessionsStore.getState().groups[0]!.broadcast).toBe(false);
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().attachSession("t-1", { sessionId: "s-b", status: "connected" });
    useSessionsStore.getState().split("row");
    useSessionsStore.getState().focusPane("a");
    useSessionsStore.getState().toggleBroadcast();
    expect(broadcastPeers("a")).toEqual(["s-b"]);
    expect(broadcastPeers("t-1")).toEqual([]);
    useSessionsStore.getState().updateTab("t-1", { status: "closed" });
    expect(broadcastPeers("a")).toEqual([]);
    await useSessionsStore.getState().closeSession("t-1");
    await useSessionsStore.getState().closeSession("t-2");
    expect(useSessionsStore.getState().groups[0]!.broadcast).toBe(false);
  });

  it("attaches sessions to picking panes only and applies an event that arrived meanwhile", () => {
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    expect(useSessionsStore.getState().attachSession("a", { sessionId: "s-x" })).toBe(false);
    expect(useSessionsStore.getState().attachSession("zz", { sessionId: "s-x" })).toBe(false);
    // An exit reported while the pane still shows the picker is buffered, then applied on attach.
    sessionEventHandlerFor("t-1")({ kind: "exit", code: 0 });
    expect(focusedPane(useSessionsStore.getState())?.status).toBe("picking");
    expect(useSessionsStore.getState().attachSession("t-1", { sessionId: "s-b", status: "connected" })).toBe(true);
    expect(focusedPane(useSessionsStore.getState())).toMatchObject({ sessionId: "s-b", status: "closed", origin: undefined });
  });

  it("replaces an ended pane with a fresh picking pane (Review Focus 2)", () => {
    useSessionsStore.getState().addTab(pane("a", "closed"));
    const fresh = useSessionsStore.getState().replacePane("a");
    expect(fresh).toBe("t-1");
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
    expect(useSessionsStore.getState().groups[0]).toMatchObject({ id: "a", layout: { paneId: "t-1" }, focusedPaneId: "t-1" });
    // A late event of the old session goes to the old id and cannot touch the new pane.
    sessionEventHandlerFor("a")({ kind: "exit", code: 0 });
    expect(focusedPane(useSessionsStore.getState())).toMatchObject({ id: "t-1", status: "picking", origin: { kind: "ssh", hostId: "h-a" } });
    discardEarlySessionEvent("a");
  });

  it("opens a local shell inside a picking pane", async () => {
    vi.mocked(api.sessionOpenLocal).mockResolvedValue("s-l");
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    await useSessionsStore.getState().openLocal({ paneId: "t-1" });
    expect(useSessionsStore.getState().groups).toHaveLength(1);
    expect(focusedPane(useSessionsStore.getState())).toMatchObject({ id: "t-1", kind: "local", sessionId: "s-l", status: "connected" });
    expect(getStream("t-1")).toBe(stream);
  });

  it("closes the local shell when its picking pane vanished meanwhile", async () => {
    let resolve: (id: string) => void = () => undefined;
    vi.mocked(api.sessionOpenLocal).mockImplementation(() => new Promise((r) => (resolve = r)));
    useSessionsStore.getState().addTab(pane("a"));
    useSessionsStore.getState().split("row");
    const opening = useSessionsStore.getState().openLocal({ paneId: "t-1" });
    useSessionsStore.getState().closeTab("t-1");
    resolve("s-late");
    await opening;
    expect(api.sessionClose).toHaveBeenCalledWith("s-late");
    expect(getStream("t-1")).toBeUndefined();
    expect(useSessionsStore.getState().groups).toHaveLength(1);
  });
});
```

Add this helper next to the imports. It uses the real handler factory:

```ts
import { sessionEventHandler } from "./sessionEvents";
const sessionEventHandlerFor = (paneId: string) => sessionEventHandler(paneId);
```

Leftover buffered events must not leak into later tests that reuse an id: the replace test ends with
`discardEarlySessionEvent("a");`.

- [ ] **Step 3: Run and verify failure**

Run: `bun run test ui/src/stores/sessions.test.ts 2>&1 | tail -8`
Expected: FAIL (`split is not a function`, `groups` undefined, missing exports).

- [ ] **Step 4: Implement**

Add to `ui/src/test/fixtures.ts` (import `TabGroup` and `Tab` types from `../stores/sessions`):

```ts
/** One single-pane group per tab, as `addTab` creates them. */
export function singleGroups(tabs: Tab[]): TabGroup[] {
  return tabs.map((tab) => ({ id: tab.id, layout: { type: "pane", paneId: tab.id }, focusedPaneId: tab.id, broadcast: false }));
}
```

`ui/src/stores/sessionEvents.ts`: buffer events for panes that are still picking. Change the handler body to:

```ts
  return (event) => {
    const sessions = useSessionsStore.getState();
    // A picking pane has no session yet: keep the event for `attachSession`.
    if (sessions.tabs.some((tab) => tab.id === tabId && tab.status !== "picking")) {
      sessions.updateTab(tabId, sessionEventPatch(event));
    } else early.set(tabId, event);
  };
```

Update the module doc as well: "…before their tab existed or while it still showed the pane picker…".

Replace `ui/src/stores/sessions.ts` with:

```ts
import { create } from "zustand";
import { t } from "../i18n/de";
import {
  type CoreError,
  createEventChannel,
  createOutputStream,
  type Id,
  newId,
  type SessionEventDto,
  type SessionId,
  type SessionInfo,
  sessionClose,
  sessionOpenLocal,
} from "../ipc/api";
import { errorDetails } from "../ipc/errors";
import {
  type Layout,
  leaf,
  MAX_PANES,
  neighbour,
  paneIds,
  removePane,
  renamePane,
  type SplitDir,
  type SplitPath,
  setRatio as setLayoutRatio,
  splitPane,
} from "../screens/terminal/layout";
import { pushNotice } from "./notices";
import { discardEarlySessionEvent, sessionEventHandler, sessionEventPatch, takeEarlySessionEvent } from "./sessionEvents";
import { useSettingsStore } from "./settings";
import { dropStream, registerStream } from "./streams";
import { estimateTerminalSize } from "./terminalSize";
import { useUiStore } from "./ui";

export type TabKind = "ssh" | "local";
/** `picking`: a new split pane that shows the host/local picker and has no session yet. */
export type TabStatus = "picking" | "connecting" | "connected" | "closed" | "error";

/** One terminal pane = one session. Grouped into UI tabs by `TabGroup`. */
export interface Tab {
  id: string;
  title: string;
  kind: TabKind;
  hostId?: Id;
  sessionId?: SessionId;
  status: TabStatus;
  /** Why the session ended, for `status: "error"`. */
  error?: CoreError;
  /** Negotiated algorithms etc. of an SSH session (sidebar "Sitzung"). */
  info?: SessionInfo;
  /** Client time (ms) when the session connected / the local terminal opened. */
  connectedAt?: number;
  /** Current terminal size, reported by `TerminalView`. */
  cols?: number;
  rows?: number;
  /** Picking panes: the pane it was split from (offered as "Gleicher Host"). */
  origin?: { kind: TabKind; hostId?: Id };
}

/** A UI tab: the split layout of its panes. Its id is the id of its first pane. */
export interface TabGroup {
  id: string;
  layout: Layout;
  focusedPaneId: string;
  broadcast: boolean;
}

export interface SessionsState {
  /** All panes of all groups. */
  tabs: Tab[];
  groups: TabGroup[];
  /** Active group id. */
  activeId: string | null;
  /** Adds the pane as a new single-pane group and activates it; applies an early session event. */
  addTab(tab: Tab): void;
  /** Removes the pane from its group; the group goes with its last pane (then a neighbour group activates). */
  closeTab(id: string): void;
  setActive(groupId: string): void;
  updateTab(id: string, patch: Partial<Omit<Tab, "id">>): void;
  /** Opens a local terminal as a new tab, or inside picking pane `target.paneId`; failures become a notice. */
  openLocal(target?: { paneId: string }): Promise<void>;
  /** Removes the pane and closes its backend session (best effort). */
  closeSession(id: string): Promise<void>;
  /** Closes every pane (and session) of the group. */
  closeGroup(groupId: string): Promise<void>;
  /** Splits the focused pane of the active group; the new pane picks what to open. No-op at `MAX_PANES`. */
  split(dir: SplitDir): void;
  focusPane(paneId: string): void;
  focusNeighbour(step: 1 | -1): void;
  setRatio(groupId: string, path: SplitPath, ratio: number): void;
  /** Toggles broadcast of the active group; only with at least two panes. */
  toggleBroadcast(): void;
  /** Gives a picking pane its session. False if the pane is gone or not picking (caller closes the session). */
  attachSession(paneId: string, patch: Partial<Omit<Tab, "id">>): boolean;
  /**
   * Swaps the pane for a fresh picking pane (new id, same place) and closes the old session. A late event
   * of the old session then targets the old id and cannot end whatever connects in the new pane.
   */
  replacePane(paneId: string): string;
}

const singleGroup = (paneId: string): TabGroup => ({ id: paneId, layout: leaf(paneId), focusedPaneId: paneId, broadcast: false });

const groupOfPane = (groups: readonly TabGroup[], paneId: string) =>
  groups.find((group) => paneIds(group.layout).includes(paneId));

export function activeGroup(state: Pick<SessionsState, "groups" | "activeId">): TabGroup | undefined {
  return state.groups.find((group) => group.id === state.activeId);
}

/** The focused pane of the active group (the sidebar's subject). */
export function focusedPane(state: Pick<SessionsState, "groups" | "activeId" | "tabs">): Tab | undefined {
  const group = activeGroup(state);
  return group ? state.tabs.find((tab) => tab.id === group.focusedPaneId) : undefined;
}

/** Sessions that also receive `paneId`'s input: only while it is the focused pane of a broadcasting group. */
export function broadcastPeers(paneId: string): SessionId[] {
  const { tabs, groups } = useSessionsStore.getState();
  const group = groups.find((candidate) => candidate.broadcast && candidate.focusedPaneId === paneId);
  if (!group) return [];
  const ids = paneIds(group.layout);
  return tabs
    .filter((tab) => tab.id !== paneId && ids.includes(tab.id) && tab.status === "connected" && tab.sessionId)
    .map((tab) => tab.sessionId as SessionId);
}

function withoutPane(state: SessionsState, paneId: string): Partial<SessionsState> {
  const tabs = state.tabs.filter((tab) => tab.id !== paneId);
  const index = state.groups.findIndex((group) => paneIds(group.layout).includes(paneId));
  const group = state.groups[index];
  if (!group) return { tabs };
  const layout = removePane(group.layout, paneId);
  if (layout) {
    const focusedPaneId = group.focusedPaneId === paneId ? neighbour(group.layout, paneId, -1) : group.focusedPaneId;
    const broadcast = group.broadcast && paneIds(layout).length > 1;
    return { tabs, groups: state.groups.map((g) => (g === group ? { ...g, layout, focusedPaneId, broadcast } : g)) };
  }
  const groups = state.groups.filter((g) => g !== group);
  if (state.activeId !== group.id) return { tabs, groups };
  const next = groups[index] ?? groups[index - 1];
  return { tabs, groups, activeId: next ? next.id : null };
}

async function closeQuietly(sessionId: SessionId | undefined): Promise<void> {
  if (!sessionId) return;
  try {
    await sessionClose(sessionId);
  } catch {
    // the session already ended
  }
}

export const useSessionsStore = create<SessionsState>()((set, get) => {
  const updateGroup = (groupId: string, change: (group: TabGroup) => TabGroup) =>
    set((state) => ({ groups: state.groups.map((group) => (group.id === groupId ? change(group) : group)) }));

  return {
    tabs: [],
    groups: [],
    activeId: null,
    addTab: (tab) => {
      const early = takeEarlySessionEvent(tab.id);
      const added = early ? { ...tab, ...sessionEventPatch(early) } : tab;
      set((state) => ({ tabs: [...state.tabs, added], groups: [...state.groups, singleGroup(added.id)], activeId: added.id }));
    },
    closeTab: (id) => set((state) => withoutPane(state, id)),
    setActive: (activeId) => set({ activeId }),
    updateTab: (id, patch) =>
      set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === id ? { ...tab, ...patch } : tab)) })),
    openLocal: async (target) => {
      const tabId = target?.paneId ?? newId();
      const stream = createOutputStream();
      const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
      const { cols, rows } = estimateTerminalSize(useSettingsStore.getState().settings.terminalFontSize);
      try {
        const sessionId = await sessionOpenLocal(cols, rows, stream.channel, sessionEvents);
        stream.bind(sessionId);
        registerStream(tabId, stream);
        const count = get().tabs.filter((tab) => tab.kind === "local" && tab.status !== "picking").length;
        const title = count === 0 ? t.terminal.localTitle : `${t.terminal.localTitle} ${count + 1}`;
        const opened = { title, kind: "local", sessionId, status: "connected", connectedAt: Date.now() } as const;
        if (!target) get().addTab({ id: tabId, ...opened });
        else if (!get().attachSession(tabId, opened)) {
          dropStream(tabId);
          discardEarlySessionEvent(tabId);
          await closeQuietly(sessionId);
          return;
        }
        useUiStore.getState().setView("terminal");
      } catch (error) {
        discardEarlySessionEvent(tabId);
        pushNotice({ tone: "red", message: t.terminal.localFailed, details: errorDetails(error) });
      }
    },
    closeSession: async (id) => {
      const tab = get().tabs.find((candidate) => candidate.id === id);
      get().closeTab(id);
      dropStream(id);
      await closeQuietly(tab?.sessionId);
    },
    closeGroup: async (groupId) => {
      const group = get().groups.find((candidate) => candidate.id === groupId);
      if (!group) return;
      await Promise.all(paneIds(group.layout).map((paneId) => get().closeSession(paneId)));
    },
    split: (dir) => {
      const state = get();
      const group = activeGroup(state);
      if (!group || paneIds(group.layout).length >= MAX_PANES) return;
      const source = state.tabs.find((tab) => tab.id === group.focusedPaneId);
      const origin = source?.status === "picking" ? source.origin : source && { kind: source.kind, hostId: source.hostId };
      const paneId = newId();
      const pane: Tab = { id: paneId, title: t.terminal.panes.newPane, kind: origin?.kind ?? "local", status: "picking", origin };
      set((current) => ({
        tabs: [...current.tabs, pane],
        groups: current.groups.map((g) =>
          g.id === group.id ? { ...g, layout: splitPane(g.layout, g.focusedPaneId, dir, paneId), focusedPaneId: paneId } : g,
        ),
      }));
    },
    focusPane: (paneId) => {
      const group = groupOfPane(get().groups, paneId);
      if (!group || group.focusedPaneId === paneId) return;
      updateGroup(group.id, (g) => ({ ...g, focusedPaneId: paneId }));
    },
    focusNeighbour: (step) => {
      const group = activeGroup(get());
      if (group) get().focusPane(neighbour(group.layout, group.focusedPaneId, step));
    },
    setRatio: (groupId, path, ratio) => updateGroup(groupId, (g) => ({ ...g, layout: setLayoutRatio(g.layout, path, ratio) })),
    toggleBroadcast: () => {
      const group = activeGroup(get());
      if (!group || (!group.broadcast && paneIds(group.layout).length < 2)) return;
      updateGroup(group.id, (g) => ({ ...g, broadcast: !g.broadcast }));
    },
    attachSession: (paneId, patch) => {
      const pane = get().tabs.find((tab) => tab.id === paneId);
      if (pane?.status !== "picking") return false;
      const early = takeEarlySessionEvent(paneId);
      get().updateTab(paneId, { ...patch, origin: undefined, ...(early ? sessionEventPatch(early) : {}) });
      return true;
    },
    replacePane: (paneId) => {
      const pane = get().tabs.find((tab) => tab.id === paneId);
      if (!pane) return paneId;
      const freshId = newId();
      const fresh: Tab = {
        id: freshId,
        title: pane.title,
        kind: pane.kind,
        status: "picking",
        origin: { kind: pane.kind, hostId: pane.hostId },
      };
      set((state) => ({
        tabs: state.tabs.map((tab) => (tab.id === paneId ? fresh : tab)),
        groups: state.groups.map((g) =>
          paneIds(g.layout).includes(paneId)
            ? { ...g, layout: renamePane(g.layout, paneId, freshId), focusedPaneId: g.focusedPaneId === paneId ? freshId : g.focusedPaneId }
            : g,
        ),
      }));
      dropStream(paneId);
      void closeQuietly(pane.sessionId);
      return freshId;
    },
  };
});
```

- [ ] **Step 5: Fix the existing seeds**

`ui/src/screens/terminal/TerminalScreen.test.tsx`: change `seed` to also set the groups, and add a
`closeGroup` mock:

```ts
const closeGroup = vi.fn(async () => undefined);

function seed(tabs: Tab[], activeId: string | null) {
  useSessionsStore.setState({ tabs, groups: singleGroups(tabs), activeId, openLocal, closeSession, closeGroup });
}
```

Import `singleGroups` from `../../test/fixtures`.

- [ ] **Step 6: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -6`
Expected: everything passes. `TerminalScreen` still renders `tabs` until Task 5; with single-pane groups the group id
equals the pane id, so nothing changes yet. The existing `stores.test.ts` test "activates new tabs and a neighbour
when the active tab closes" passes unchanged.

- [ ] **Step 7: Commit**

```bash
git add ui/src/stores ui/src/test/fixtures.ts ui/src/i18n/de.ts ui/src/screens/terminal/TerminalScreen.test.tsx
git commit -m "feat(ui): tab groups with split layout, focus and broadcast state

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Connect into a pane, reconnect in place

**Files:**
- Modify: `ui/src/stores/connect.ts`, `ui/src/screens/terminal/SessionEnded.tsx`
- Test: `ui/src/stores/connect.test.ts`, `ui/src/screens/terminal/TerminalScreen.test.tsx`

**Interfaces:**
- Consumes: Task 2 `attachSession`, `replacePane`, `split`, `addTab`, `closeTab`.
- Produces:
  - `ConnectFlow.paneId?: string`.
  - `ConnectState.start(hostId, protocol, target?: { paneId: string })`.

- [ ] **Step 1: Write the failing tests**

Append to `ui/src/stores/connect.test.ts`:

```ts
describe("connect into a pane", () => {
  const splitPicking = () => {
    useSessionsStore.getState().addTab({ id: "p0", title: "web", kind: "ssh", hostId: "h-web", sessionId: "s-0", status: "connected" });
    vi.mocked(api.newId).mockReturnValueOnce("pane-1");
    useSessionsStore.getState().split("row");
  };

  it("puts the session into the picking pane instead of a new tab", async () => {
    splitPicking();
    const started = useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    attempts[0]?.resolve("s-1");
    await started;
    expect(useSessionsStore.getState().groups).toHaveLength(1);
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === "pane-1")).toMatchObject({
      kind: "ssh",
      hostId: "h-web",
      sessionId: "s-1",
      status: "connected",
      title: "web-prod-01",
    });
    expect(getStream("pane-1")).toBe(stream);
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: null });
  });

  it("returns to the terminal and keeps the picker when cancelled (Review Focus 5)", async () => {
    splitPicking();
    void useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    await useConnectStore.getState().cancel();
    expect(useUiStore.getState().view).toBe("terminal");
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === "pane-1")?.status).toBe("picking");
  });

  it("closes the session when the pane was closed while connecting (Review Focus 3)", async () => {
    splitPicking();
    const started = useConnectStore.getState().start("h-web", "ssh", { paneId: "pane-1" });
    useSessionsStore.getState().closeTab("pane-1");
    attempts[0]?.resolve("s-late");
    await started;
    expect(api.sessionClose).toHaveBeenCalledWith("s-late");
    expect(useSessionsStore.getState().groups).toHaveLength(1);
    expect(getStream("pane-1")).toBeUndefined();
  });
});
```

In `ui/src/screens/terminal/TerminalScreen.test.tsx`, replace the test "offers close and reconnect when an SSH
session ended". Mock `api.newId` for it:

```ts
  it("offers close and reconnect in the same pane when an SSH session ended", async () => {
    vi.mocked(api.newId).mockReturnValue("p-new");
    seed([{ ...web, status: "closed" }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung beendet" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
    expect(start).toHaveBeenCalledWith("h-web", "ssh", { paneId: "p-new" });
    expect(useSessionsStore.getState().groups[0]).toMatchObject({ id: "a", focusedPaneId: "p-new" });
  });
```

Note: `seed` replaces `closeSession` with a mock, but `replacePane` is the real store action. It calls
`api.sessionClose` directly, so the assertion is on the api mock.

- [ ] **Step 2: Run and verify failure**

Run: `bun run test ui/src/stores/connect.test.ts ui/src/screens/terminal/TerminalScreen.test.tsx 2>&1 | grep -E "×|Tests"`
Expected: the new tests fail. The session lands in a new tab, cancel switches to "hosts", and reconnect calls
`start` without a target.

- [ ] **Step 3: Implement**

`ui/src/stores/connect.ts`:
- Import `dropStream` from `./streams` next to `registerStream`, and `sessionClose` from the api if it is not
  imported yet.
- Add to `ConnectFlow`:
  ```ts
    /** Split pane (status `picking`) that receives the session instead of a new tab. */
    paneId?: string;
  ```
- Change the `ConnectState.start` signature to
  `start(hostId: Id, protocol: ConnectProtocol, target?: { paneId: string }): Promise<void>;` with the doc comment
  `/** \`target.paneId\`: connect into that picking pane; cancel then stays in the terminal view. */`.
- In the `start` implementation, change the signature to `start: async (hostId, protocol, target) => {` and add
  `paneId: target?.paneId,` to the `flow` literal.
- Replace `onConnected` with:
  ```ts
  const onConnected = (flow: ConnectFlow, sessionId: string, info: SessionInfo, tabId: string, stream: OutputStream) => {
    stream.bind(sessionId);
    registerStream(tabId, stream);
    const title = useHostsStore.getState().hosts.find((h) => h.id === flow.hostId)?.name ?? flow.hostId;
    const session = { title, kind: "ssh", hostId: flow.hostId, sessionId, status: "connected", info, connectedAt: Date.now() } as const;
    const sessions = useSessionsStore.getState();
    if (!flow.paneId) sessions.addTab({ id: tabId, ...session });
    else if (!sessions.attachSession(tabId, session)) {
      // The pane was closed while connecting: no stray tab, no orphaned session.
      dropStream(tabId);
      discardEarlySessionEvent(tabId);
      fireAndForget(() => sessionClose(sessionId));
    }
    close("terminal");
  };
  ```
- In `run`, change `tabId = newId();` to `tabId = flow.paneId ?? newId();`.
- In `onFailure`, use the flow's home view for `"cancelled"` and `"hostKeyRejected"`:
  ```ts
    const home: View = get().active?.paneId ? "terminal" : "hosts";
  ```
  Declare this at the top of `onFailure`, after the `isCurrent` guard, and call `close(home)` in both branches.
- In `cancel`, use `close(flow.paneId ? "terminal" : "hosts");`.

`ui/src/screens/terminal/SessionEnded.tsx`: reconnect in place.

```tsx
  const replacePane = useSessionsStore((state) => state.replacePane);
  …
  const reconnect = () => {
    if (!hostId) return;
    const paneId = replacePane(tab.id);
    void useConnectStore.getState().start(hostId, "ssh", { paneId });
  };
```

Change the button handler to `onClick={reconnect}`. Update the component doc: "close it, or reconnect SSH sessions
in the same pane".

- [ ] **Step 4: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -6`
Expected: everything passes without warnings.

- [ ] **Step 5: Commit**

```bash
git add ui/src/stores/connect.ts ui/src/stores/connect.test.ts ui/src/screens/terminal/SessionEnded.tsx ui/src/screens/terminal/TerminalScreen.test.tsx
git commit -m "feat(ui): connect into a split pane and reconnect in place

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Broadcast input and pane focus in TerminalView

**Files:**
- Modify: `ui/src/screens/terminal/TerminalView.tsx`
- Test: `ui/src/screens/terminal/TerminalView.test.tsx`

**Interfaces:**
- Consumes: Task 2 `broadcastPeers`.
- Produces: `TerminalView({ tab, visible, focused })`. `focused` is optional and defaults to `false`; when it is
  `true` the xterm gets DOM focus.

- [ ] **Step 1: Write the failing tests**

In `TerminalView.test.tsx`:
- Add `focused = 0` and `focus() { this.focused += 1; }` to the `FakeTerminal` fake. It currently has an empty
  `focus() {}`; replace that.
- Import `singleGroups` from `../../test/fixtures`.
- Add these tests:

```ts
  it("broadcasts input of the focused pane to connected peers of its group (Review Focus 4)", () => {
    const peer: Tab = { id: "tab-2", title: "db", kind: "ssh", sessionId: "s-2", status: "connected" };
    const picking: Tab = { id: "tab-3", title: "Neuer Bereich", kind: "ssh", status: "picking" };
    useSessionsStore.setState({
      tabs: [tab, peer, picking],
      groups: [
        {
          id: tab.id,
          layout: {
            type: "split",
            dir: "row",
            ratio: 0.5,
            a: { type: "pane", paneId: tab.id },
            b: { type: "split", dir: "column", ratio: 0.5, a: { type: "pane", paneId: "tab-2" }, b: { type: "pane", paneId: "tab-3" } },
          },
          focusedPaneId: tab.id,
          broadcast: true,
        },
      ],
      activeId: tab.id,
    });
    render(<TerminalView tab={tab} visible />);
    term().data.fire("uptime\r");
    expect(api.sessionWrite).toHaveBeenCalledWith("s-1", "uptime\r");
    expect(api.sessionWrite).toHaveBeenCalledWith("s-2", "uptime\r");
    expect(api.sessionWrite).toHaveBeenCalledTimes(2);
    term().binary.fire("\u0080");
    expect(api.sessionWrite).toHaveBeenLastCalledWith("s-2", new Uint8Array([0x80]));
    // Unfocused: only its own session.
    vi.mocked(api.sessionWrite).mockClear();
    act(() => useSessionsStore.getState().focusPane("tab-2"));
    term().data.fire("x");
    expect(vi.mocked(api.sessionWrite).mock.calls).toEqual([["s-1", "x"]]);
  });

  it("does not broadcast without the flag", () => {
    useSessionsStore.setState({ tabs: [tab], groups: singleGroups([tab]), activeId: tab.id });
    render(<TerminalView tab={tab} visible />);
    term().data.fire("ls");
    expect(vi.mocked(api.sessionWrite).mock.calls).toEqual([["s-1", "ls"]]);
  });

  it("takes DOM focus when it becomes the focused pane", () => {
    const { rerender } = render(<TerminalView tab={tab} visible focused={false} />);
    const before = term().focused;
    rerender(<TerminalView tab={tab} visible focused />);
    expect(term().focused).toBe(before + 1);
  });
```

If `api.sessionWrite` is not reset between tests, add `vi.mocked(api.sessionWrite).mockClear();` to `beforeEach`.

- [ ] **Step 2: Run and verify failure**

Run: `bun run test ui/src/screens/terminal/TerminalView.test.tsx 2>&1 | grep -E "×|Tests"`
Expected: the broadcast test fails because there is no `s-2` write, and the focus test fails.

- [ ] **Step 3: Implement**

`TerminalView.tsx`:
- Import `broadcastPeers` from `../../stores/sessions` (with `type Tab` and `useSessionsStore`).
- Change the signature to
  `export function TerminalView({ tab, visible, focused = false }: { tab: Tab; visible: boolean; focused?: boolean })`.
- Inside the main effect, before `subscriptions`, add:

```ts
    // Own session first, then (broadcast) every connected peer; each session keeps its own write order.
    const send = (data: string | Uint8Array) => {
      fireAndForget(() => sessionWrite(sessionId, data));
      for (const peer of broadcastPeers(tab.id)) fireAndForget(() => sessionWrite(peer, data));
    };
```

- Change the first two subscriptions to:

```ts
      term.onData((data) => send(data)),
      term.onBinary((data) => send(Uint8Array.from(data, (char) => char.charCodeAt(0) & 0xff))),
```

- After the main effect, add:

```ts
  // The focused pane (click, Ctrl+Shift+←/→, after a split) gets the keyboard.
  useEffect(() => {
    if (focused) termRef.current?.focus();
  }, [focused, sessionId]);
```

- [ ] **Step 4: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -6`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/terminal/TerminalView.tsx ui/src/screens/terminal/TerminalView.test.tsx
git commit -m "feat(ui): broadcast terminal input to the group's connected panes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Pane layout rendering, divider, pane picker

**Files:**
- Create:
  - `ui/src/screens/terminal/PaneLayout.tsx` and `PaneLayout.module.css`
  - `ui/src/screens/terminal/PanePicker.tsx` and `PanePicker.test.tsx`
  - `ui/src/screens/terminal/PaneLayout.test.tsx`
- Modify:
  - `ui/src/screens/terminal/TerminalScreen.tsx`
  - `ui/src/screens/terminal/sidebar/Sidebar.tsx`
- Test: `ui/src/screens/terminal/TerminalScreen.test.tsx`

**Interfaces:**
- Consumes:
  - Task 1 `arrange`, `paneIds`, `MIN_RATIO` and `MAX_RATIO`.
  - Task 2 `TabGroup`, `focusPane`, `setRatio`, `closeSession`, `openLocal({ paneId })`, `activeGroup` and
    `focusedPane`.
  - Task 3 `start(hostId, "ssh", { paneId })`.
  - Task 4 `TerminalView` `focused` prop.
- Produces:
  - `PaneLayout({ group, visible })` renders all panes of a group flat.
  - `PanePicker({ pane })`.

- [ ] **Step 1: Write the failing tests**

`ui/src/screens/terminal/PanePicker.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { makeHost } from "../../test/fixtures";
import { PanePicker } from "./PanePicker";

vi.mock("../../ipc/api");

const start = vi.fn(async () => undefined);
const openLocal = vi.fn(async () => undefined);
const closeSession = vi.fn(async () => undefined);
const picking: Tab = { id: "p1", title: "Neuer Bereich", kind: "ssh", status: "picking", origin: { kind: "ssh", hostId: "h1" } };

beforeEach(() => {
  start.mockClear();
  openLocal.mockClear();
  closeSession.mockClear();
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  useSessionsStore.setState({ ...useSessionsStore.getInitialState(), openLocal, closeSession }, true);
  useHostsStore.setState(
    {
      ...useHostsStore.getInitialState(),
      hosts: [
        makeHost({ id: "h1", name: "web-prod-01", address: "10.0.1.12" }),
        makeHost({ id: "h2", name: "db-prod-01", address: "10.0.1.20" }),
        makeHost({ id: "h3", name: "files", address: "10.0.1.30", protocolSsh: false }),
      ],
    },
    true,
  );
});

const options = () => screen.getAllByRole("option").map((option) => option.textContent);

describe("PanePicker", () => {
  it("preselects the same host and opens it with Enter", async () => {
    render(<PanePicker pane={picking} />);
    expect(options()[0]).toContain("Gleicher Host (web-prod-01)");
    expect(options().some((text) => text?.includes("files"))).toBe(false);
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
    await userEvent.type(screen.getByRole("combobox", { name: "Host suchen" }), "{Enter}");
    expect(start).toHaveBeenCalledWith("h1", "ssh", { paneId: "p1" });
  });

  it("filters by name and address and picks with the arrow keys", async () => {
    render(<PanePicker pane={picking} />);
    await userEvent.type(screen.getByRole("combobox", { name: "Host suchen" }), "10.0.1.2");
    expect(options()[0]).toContain("db-prod-01");
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(openLocal).toHaveBeenCalledWith({ paneId: "p1" });
  });

  it("offers the local terminal first for a pane split from a local one, and closes on Escape", async () => {
    render(<PanePicker pane={{ ...picking, kind: "local", origin: { kind: "local" } }} />);
    expect(options()[0]).toBe("Lokales Terminal");
    await userEvent.click(screen.getByRole("option", { name: /db-prod-01/ }));
    expect(start).toHaveBeenCalledWith("h2", "ssh", { paneId: "p1" });
    await userEvent.type(screen.getByRole("combobox", { name: "Host suchen" }), "{Escape}");
    expect(closeSession).toHaveBeenCalledWith("p1");
  });
});
```

`ui/src/screens/terminal/PaneLayout.test.tsx`:

```tsx
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { PaneLayout } from "./PaneLayout";

vi.mock("../../ipc/api");
const mounts: string[] = [];
vi.mock("./TerminalView", async () => {
  const { useEffect } = await import("react");
  return {
    TerminalView: ({ tab, focused }: { tab: Tab; focused?: boolean }) => {
      useEffect(() => {
        mounts.push(tab.id);
      }, [tab.id]);
      return <div data-testid={`term-${tab.id}`} data-focused={String(!!focused)} />;
    },
  };
});

const a: Tab = { id: "a", title: "web", kind: "ssh", hostId: "h", sessionId: "s-a", status: "connected" };

beforeEach(() => {
  mounts.length = 0;
  vi.mocked(api.newId).mockReturnValue("p-2");
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useSessionsStore.getState().addTab(a);
});

const group = () => useSessionsStore.getState().groups[0]!;

describe("PaneLayout", () => {
  it("splits without remounting the running terminal (Review Focus 1)", () => {
    const { rerender } = render(<PaneLayout group={group()} visible />);
    act(() => useSessionsStore.getState().split("row"));
    rerender(<PaneLayout group={group()} visible />);
    expect(mounts).toEqual(["a"]);
    expect(screen.getByRole("combobox", { name: "Host suchen" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Bereich schließen" })).toHaveLength(2);
  });

  it("focuses a pane on mouse down and moves the divider with the keyboard", () => {
    act(() => useSessionsStore.getState().split("row"));
    render(<PaneLayout group={group()} visible />);
    fireEvent.mouseDown(screen.getByTestId("term-a"));
    expect(group().focusedPaneId).toBe("a");
    const separator = screen.getByRole("separator", { name: "Bereichsgröße" });
    expect(separator).toHaveAttribute("aria-orientation", "vertical");
    expect(separator).toHaveAttribute("aria-valuenow", "50");
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(group().layout).toMatchObject({ ratio: 0.55 });
  });

  it("shows a single pane without frame controls", () => {
    render(<PaneLayout group={group()} visible />);
    expect(screen.queryByRole("button", { name: "Bereich schließen" })).toBeNull();
    expect(screen.queryByRole("separator")).toBeNull();
    expect(screen.getByTestId("term-a")).toHaveAttribute("data-focused", "true");
  });
});
```

In `TerminalScreen.test.tsx`, extend "toggles the sidebar …" or add a test showing that the sidebar follows the
focused pane and shows "Kein Terminal" for a picking pane:

```ts
  it("shows the focused pane in the sidebar; a picking pane has no terminal", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalSidebarOpen: true } });
    seed([web], "a");
    vi.mocked(api.newId).mockReturnValue("p-2");
    act(() => useSessionsStore.getState().split("row"));
    render(<TerminalScreen />);
    const sidebar = screen.getByRole("complementary", { name: "Sitzungsdetails" });
    expect(within(sidebar).getByText("Kein Terminal")).toBeInTheDocument();
  });
```

Import `act` from `@testing-library/react` in that file.

- [ ] **Step 2: Run and verify failure**

Run: `bun run test ui/src/screens/terminal 2>&1 | grep -E "FAIL|×|Tests"`
Expected: FAIL, because `./PanePicker` and `./PaneLayout` cannot be resolved and "Kein Terminal" is missing.

- [ ] **Step 3: Implement `PanePicker.tsx`**

```tsx
import { useEffect, useId, useRef, useState } from "react";
import { cx } from "../../components";
import { t } from "../../i18n/de";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import styles from "./PaneLayout.module.css";

interface Choice {
  key: string;
  label: string;
  detail?: string;
  open(): void;
}

/** What a new split pane opens: same host (preselected), another SSH host, or a local shell. */
export function PanePicker({ pane }: { pane: Tab }) {
  const texts = t.terminal.panes;
  const hosts = useHostsStore((state) => state.hosts);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const closeSession = useSessionsStore((state) => state.closeSession);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useId();

  useEffect(() => inputRef.current?.focus(), []);

  const connect = (hostId: string) => void useConnectStore.getState().start(hostId, "ssh", { paneId: pane.id });
  const local: Choice = { key: "local", label: texts.local, open: () => void openLocal({ paneId: pane.id }) };
  const origin = pane.origin?.kind === "ssh" ? hosts.find((host) => host.id === pane.origin?.hostId) : undefined;
  const needle = query.trim().toLowerCase();
  const hostChoices: Choice[] = hosts
    .filter((host) => host.protocolSsh)
    .filter((host) => !needle || host.name.toLowerCase().includes(needle) || host.address.toLowerCase().includes(needle))
    .map((host) => ({ key: host.id, label: host.name, detail: host.address, open: () => connect(host.id) }));
  const first: Choice = origin
    ? { key: "same", label: texts.sameHost(origin.name), detail: origin.address, open: () => connect(origin.id) }
    : local;
  const choices = needle ? [...hostChoices, local] : [first, ...hostChoices, ...(first === local ? [] : [local])];
  const selected = Math.min(index, choices.length - 1);
  const optionId = (key: string) => `${listId}-${key}`;

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setIndex((selected + step + choices.length) % choices.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      choices[selected]?.open();
    } else if (event.key === "Escape") {
      event.preventDefault();
      void closeSession(pane.id);
    }
  };

  return (
    <div className={styles.picker}>
      <p className={styles.pickerTitle}>{texts.choose}</p>
      <input
        ref={inputRef}
        className={styles.search}
        role="combobox"
        aria-label={texts.search}
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={choices[selected] ? optionId(choices[selected].key) : undefined}
        placeholder={texts.search}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setIndex(0);
        }}
        onKeyDown={onKeyDown}
      />
      <ul id={listId} role="listbox" aria-label={texts.choices} className={styles.choices}>
        {choices.map((choice, position) => (
          <li
            key={choice.key}
            id={optionId(choice.key)}
            role="option"
            aria-selected={position === selected}
            className={cx(styles.choice, position === selected && styles.selected)}
            onMouseDown={(event) => event.preventDefault()}
            onClick={choice.open}
          >
            <span>{choice.label}</span>
            {choice.detail ? <span className={styles.detail}>{choice.detail}</span> : null}
          </li>
        ))}
      </ul>
    </div>
  );
}
```

If the project's tsconfig does not allow the global `React` namespace, import `type KeyboardEvent` from "react" and
type the handler parameter with it.

- [ ] **Step 4: Implement `PaneLayout.tsx` and its CSS**

```tsx
import { X } from "lucide-react";
import { type KeyboardEvent, type PointerEvent, useRef } from "react";
import { cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { type TabGroup, useSessionsStore } from "../../stores/sessions";
import { arrange, paneIds, type Rect, type SplitDir, type SplitPath } from "./layout";
import styles from "./PaneLayout.module.css";
import { PanePicker } from "./PanePicker";
import { SessionEnded } from "./SessionEnded";
import { TerminalView } from "./TerminalView";

const pct = (value: number) => `${value * 100}%`;
const box = (rect: Rect) => ({ left: pct(rect.x), top: pct(rect.y), width: pct(rect.w), height: pct(rect.h) });
const KEY_STEP = 0.05;

/**
 * All panes of one tab, rendered flat (keyed by pane id) at positions from `arrange`: changing the tree
 * never remounts a running terminal.
 */
export function PaneLayout({ group, visible }: { group: TabGroup; visible: boolean }) {
  const areaRef = useRef<HTMLDivElement>(null);
  const { panes, dividers } = arrange(group.layout);
  const multi = paneIds(group.layout).length > 1;
  return (
    <div ref={areaRef} className={cx(styles.area, group.broadcast && styles.broadcasting)}>
      {panes.map(({ paneId, rect }) => (
        <Pane key={paneId} paneId={paneId} group={group} rect={rect} multi={multi} visible={visible} />
      ))}
      {dividers.map((divider) => (
        <Divider key={divider.path.join("") || "root"} groupId={group.id} areaRef={areaRef} {...divider} />
      ))}
    </div>
  );
}

function Pane({ paneId, group, rect, multi, visible }: { paneId: string; group: TabGroup; rect: Rect; multi: boolean; visible: boolean }) {
  const pane = useSessionsStore((state) => state.tabs.find((tab) => tab.id === paneId));
  const focusPane = useSessionsStore((state) => state.focusPane);
  const closeSession = useSessionsStore((state) => state.closeSession);
  if (!pane) return null;
  const focused = group.focusedPaneId === paneId;
  const ended = pane.status === "closed" || pane.status === "error";
  return (
    <div
      className={cx(styles.pane, multi && focused && styles.focused, group.broadcast && styles.broadcast)}
      style={box(rect)}
      data-pane={paneId}
      onMouseDown={() => focusPane(paneId)}
      onFocus={() => focusPane(paneId)}
    >
      {pane.status === "picking" ? <PanePicker pane={pane} /> : null}
      {pane.sessionId ? <TerminalView tab={pane} visible={visible} focused={visible && focused} /> : null}
      {ended ? <SessionEnded tab={pane} /> : null}
      {multi ? (
        <IconButton
          size="sm"
          className={styles.close}
          label={t.terminal.panes.close}
          icon={X}
          onClick={() => void closeSession(paneId)}
        />
      ) : null}
    </div>
  );
}

function Divider({
  groupId,
  areaRef,
  path,
  dir,
  ratio,
  area,
}: {
  groupId: string;
  areaRef: React.RefObject<HTMLDivElement | null>;
  path: SplitPath;
  dir: SplitDir;
  ratio: number;
  area: Rect;
}) {
  const setRatio = useSessionsStore((state) => state.setRatio);
  const row = dir === "row";
  const style = row
    ? { left: pct(area.x + area.w * ratio), top: pct(area.y), height: pct(area.h) }
    : { top: pct(area.y + area.h * ratio), left: pct(area.x), width: pct(area.w) };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const bounds = areaRef.current?.getBoundingClientRect();
    if (!bounds) return;
    event.preventDefault();
    const target = event.currentTarget;
    target.setPointerCapture?.(event.pointerId);
    const move = (moveEvent: globalThis.PointerEvent) => {
      const fraction = row
        ? ((moveEvent.clientX - bounds.left) / bounds.width - area.x) / area.w
        : ((moveEvent.clientY - bounds.top) / bounds.height - area.y) / area.h;
      setRatio(groupId, path, fraction);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const grow = row ? event.key === "ArrowRight" : event.key === "ArrowDown";
    const shrink = row ? event.key === "ArrowLeft" : event.key === "ArrowUp";
    if (!grow && !shrink) return;
    event.preventDefault();
    setRatio(groupId, path, Math.round((ratio + (grow ? KEY_STEP : -KEY_STEP)) * 100) / 100);
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable, draggable splitter has no native element
    <div
      role="separator"
      aria-label={t.terminal.panes.separator}
      aria-orientation={row ? "vertical" : "horizontal"}
      aria-valuenow={Math.round(ratio * 100)}
      aria-valuemin={15}
      aria-valuemax={85}
      tabIndex={0}
      className={cx(styles.divider, row ? styles.dividerRow : styles.dividerColumn)}
      style={style}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    />
  );
}
```

If the global `React` namespace is not available, import `type RefObject` from "react" and use it instead of
`React.RefObject`. The biome-ignore comment is harmless if biome is not used; remove it if it causes noise.

`ui/src/screens/terminal/PaneLayout.module.css`:

```css
.area {
  position: absolute;
  inset: 0;
}

.pane {
  position: absolute;
  display: flex;
  min-width: 0;
  min-height: 0;
  box-sizing: border-box;
  border: 1px solid transparent;
}

.focused {
  border-color: var(--accent);
}

.broadcast {
  border-color: var(--amber);
}

.close {
  position: absolute;
  top: 4px;
  right: 4px;
  z-index: 2;
  opacity: 0.6;
}

.close:hover,
.close:focus-visible {
  opacity: 1;
}

.divider {
  position: absolute;
  z-index: 3;
  background: transparent;
}

.divider:hover,
.divider:focus-visible {
  background: var(--accent);
  outline: none;
}

.dividerRow {
  width: 6px;
  transform: translateX(-50%);
  cursor: col-resize;
}

.dividerColumn {
  height: 6px;
  transform: translateY(-50%);
  cursor: row-resize;
}

.picker {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(360px, 90%);
  margin: auto;
}

.pickerTitle {
  margin: 0;
  color: var(--text);
  font-size: 14px;
  font-weight: 500;
}

.search {
  height: 36px;
  padding: 0 10px;
  border: 1px solid var(--bd);
  border-radius: 8px;
  background: var(--s1);
  color: var(--text);
  font: inherit;
}

.choices {
  display: flex;
  flex-direction: column;
  max-height: 240px;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.choice {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13px;
  cursor: pointer;
}

.selected {
  background: var(--card);
}

.detail {
  color: var(--muted);
  font-family: "JetBrains Mono", monospace;
  font-size: 12px;
}
```

Check the tokens first with `grep -n "\-\-amber\|\-\-card\|\-\-s1" ui/src/styles/tokens.css`. Use the closest existing
warning color if `--amber` is missing, for example `--yellow` or `--orange`.

- [ ] **Step 5: Wire `TerminalScreen` and `Sidebar`**

`TerminalScreen.tsx`: render one tabpanel per **group**, and pass the focused pane to the sidebar.

```tsx
import { activeGroup, focusedPane, useSessionsStore } from "../../stores/sessions";
import { PaneLayout } from "./PaneLayout";
…
  const groups = useSessionsStore((state) => state.groups);
  const activeId = useSessionsStore((state) => state.activeId);
  const pane = useSessionsStore((state) => focusedPane(state));
  …
      {groups.length === 0 ? (
        … empty state unchanged …
      ) : (
        <>
          <TabBar />
          <div className={styles.body}>
            <div className={styles.panels}>
              {groups.map((group) => (
                <div
                  key={group.id}
                  role="tabpanel"
                  id={tabPanelId(group.id)}
                  aria-labelledby={tabButtonId(group.id)}
                  hidden={group.id !== activeId}
                  className={styles.panel}
                >
                  <PaneLayout group={group} visible={shown && group.id === activeId} />
                </div>
              ))}
            </div>
            {sidebarOpen && pane ? <Sidebar tab={pane} /> : null}
          </div>
        </>
      )}
```

Remove the now-unused imports (`TerminalView`, `SessionEnded`, `tabs`, and `activeGroup` if unused). Update the
component doc: "tab bar + one panel per tab (group) with its split panes".

`Sidebar.tsx`: a picking pane has no terminal yet. Replace `<Panel tab={tab} />` with:

```tsx
        {tab.status === "picking" ? <p className={styles.hint}>{t.terminal.panes.noTerminal}</p> : <Panel tab={tab} />}
```

- [ ] **Step 6: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -6`
Expected: everything passes without act() or other warnings. The existing `TerminalScreen` tests still find
`term-a` and `term-b` through the mocked `TerminalView`, now rendered via `PaneLayout`.

- [ ] **Step 7: Commit**

```bash
git add ui/src/screens/terminal
git commit -m "feat(ui): render split panes flat with draggable dividers and a pane picker

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Tab bar for groups, split and broadcast buttons

**Files:**
- Modify: `ui/src/screens/terminal/TabBar.tsx`, `ui/src/screens/terminal/TabBar.module.css`
- Test: `ui/src/screens/terminal/TerminalScreen.test.tsx`

**Interfaces:**
- Consumes: Task 2 `groups`, `activeId`, `setActive`, `closeGroup`, `split`, `toggleBroadcast`, `paneIds` and
  `MAX_PANES`.

- [ ] **Step 1: Write the failing tests**

In `TerminalScreen.test.tsx`, change the existing "opens local terminals and closes tabs from the tab bar"
expectation from `closeSession` to `closeGroup`:

```ts
    await userEvent.click(screen.getByRole("button", { name: "web-prod-01 schließen" }));
    expect(closeGroup).toHaveBeenCalledWith("a");
```

Add:

```ts
  it("splits, counts panes, limits to 4 and toggles broadcast from the tab bar", async () => {
    seed([web], "a");
    let next = 0;
    vi.mocked(api.newId).mockImplementation(() => `p-${++next}`);
    render(<TerminalScreen />);
    const broadcast = screen.getByRole("button", { name: "Broadcast" });
    expect(broadcast).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Nebeneinander teilen" }));
    expect(screen.getByRole("tab", { name: "web-prod-01 · 2" })).toBeInTheDocument();
    await userEvent.click(broadcast);
    expect(broadcast).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Broadcast an · 2 Bereiche")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Untereinander teilen" }));
    await userEvent.click(screen.getByRole("button", { name: "Untereinander teilen" }));
    expect(screen.getByRole("button", { name: "Nebeneinander teilen" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Untereinander teilen" })).toBeDisabled();
  });
```

The tab name stays the focused pane's title. After splitting, the focused pane is the new picking pane titled
"Neuer Bereich". So adjust the expectation to `"Neuer Bereich · 2"`, unless the implementation below takes the title
from the first non-picking pane. Use the rule below and keep the test as written:

> Title = the focused pane's title, unless that pane is picking; then the title of the first pane in tree order that
> is not picking.

- [ ] **Step 2: Run and verify failure**

Run: `bun run test ui/src/screens/terminal/TerminalScreen.test.tsx 2>&1 | grep -E "×|Tests"`
Expected: FAIL, because there are no split or broadcast buttons and the close button calls `closeSession`.

- [ ] **Step 3: Implement the `TabBar`**

```tsx
import { Columns2, PanelRight, Plus, RadioTower, Rows2, X } from "lucide-react";
import { type KeyboardEvent, useRef } from "react";
import { Button, cx, IconButton, nextIndex } from "../../components";
import { t } from "../../i18n/de";
import { activeGroup, type Tab, type TabGroup, useSessionsStore } from "../../stores/sessions";
import { useSettingsStore } from "../../stores/settings";
import { MAX_PANES, paneIds } from "./layout";
import { toggleTerminalSidebar } from "./sidebar/toggle";
import styles from "./TabBar.module.css";

export const tabButtonId = (id: string) => `session-tab-${id}`;
export const tabPanelId = (id: string) => `session-panel-${id}`;

/** The pane a tab is named after: the focused one, unless it is still picking. */
function titlePane(group: TabGroup, tabs: readonly Tab[]): Tab | undefined {
  const panes = paneIds(group.layout).map((id) => tabs.find((tab) => tab.id === id));
  const focused = tabs.find((tab) => tab.id === group.focusedPaneId);
  return focused?.status !== "picking" ? focused : (panes.find((tab) => tab && tab.status !== "picking") ?? focused);
}

/** One tab per group plus split, broadcast, new local terminal and sidebar buttons. */
export function TabBar() {
  const tabs = useSessionsStore((state) => state.tabs);
  const groups = useSessionsStore((state) => state.groups);
  const activeId = useSessionsStore((state) => state.activeId);
  const active = useSessionsStore((state) => activeGroup(state));
  const setActive = useSessionsStore((state) => state.setActive);
  const closeGroup = useSessionsStore((state) => state.closeGroup);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const split = useSessionsStore((state) => state.split);
  const toggleBroadcast = useSessionsStore((state) => state.toggleBroadcast);
  const sidebarOpen = useSettingsStore((state) => state.settings.terminalSidebarOpen);
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const texts = t.terminal.panes;
  const activeCount = active ? paneIds(active.layout).length : 0;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, groups.length, "horizontal");
    const group = next === null ? undefined : groups[next];
    if (next === null || !group) return;
    event.preventDefault();
    setActive(group.id);
    refs.current[next]?.focus();
  };

  return (
    <div className={styles.bar}>
      <div role="tablist" aria-label={t.terminal.sessions} className={styles.tabs}>
        {groups.map((group, index) => {
          const pane = titlePane(group, tabs);
          const title = pane?.title ?? "";
          const count = paneIds(group.layout).length;
          const label = count > 1 ? texts.count(title, count) : title;
          const selected = group.id === activeId;
          const ended = pane?.status === "closed" || pane?.status === "error";
          return (
            <div key={group.id} className={cx(styles.tab, selected && styles.active)} data-status={pane?.status} data-kind={pane?.kind}>
              <button
                ref={(element) => {
                  refs.current[index] = element;
                }}
                type="button"
                role="tab"
                id={tabButtonId(group.id)}
                aria-selected={selected}
                aria-controls={tabPanelId(group.id)}
                tabIndex={selected ? 0 : -1}
                className={styles.tabButton}
                onClick={() => setActive(group.id)}
                onKeyDown={(event) => onKeyDown(event, index)}
              >
                <span className={styles.dot} aria-hidden="true" />
                <span className={styles.title}>{label}</span>
                {ended ? <span className="visually-hidden">{` – ${t.terminal.endedSuffix}`}</span> : null}
              </button>
              <IconButton
                size="sm"
                label={t.terminal.closeTab(title)}
                icon={X}
                className={styles.close}
                onClick={() => void closeGroup(group.id)}
              />
            </div>
          );
        })}
      </div>
      {active?.broadcast ? <span className={styles.broadcastState}>{texts.broadcastOn(activeCount)}</span> : null}
      <IconButton
        label={texts.splitRow}
        title={texts.splitRowTitle}
        icon={Columns2}
        disabled={!active || activeCount >= MAX_PANES}
        onClick={() => split("row")}
      />
      <IconButton
        label={texts.splitColumn}
        title={texts.splitColumnTitle}
        icon={Rows2}
        disabled={!active || activeCount >= MAX_PANES}
        onClick={() => split("column")}
      />
      <Button
        size="sm"
        variant="ghost"
        icon={RadioTower}
        title={texts.broadcastTitle}
        aria-pressed={!!active?.broadcast}
        disabled={activeCount < 2}
        className={cx(styles.broadcastButton, active?.broadcast && styles.broadcastOn)}
        onClick={toggleBroadcast}
      >
        {texts.broadcast}
      </Button>
      <IconButton label={t.terminal.newLocal} title={t.terminal.newLocalTitle} icon={Plus} onClick={() => void openLocal()} />
      <IconButton label={t.terminal.sidebar.toggle} icon={PanelRight} aria-pressed={sidebarOpen} onClick={toggleTerminalSidebar} />
    </div>
  );
}
```

Append to `TabBar.module.css`:

```css
.tab[data-status="picking"] {
  --tab-tone: var(--muted);
}

.broadcastState {
  align-self: center;
  margin-right: 6px;
  color: var(--amber);
  font-size: 12px;
}

.broadcastButton {
  align-self: center;
}

.broadcastOn {
  background: color-mix(in srgb, var(--amber) 18%, transparent);
  color: var(--amber);
}
```

Use the same warning token as in Task 5.

- [ ] **Step 4: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -6`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/terminal
git commit -m "feat(ui): tab bar per group with split and broadcast controls

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Keyboard shortcuts

**Files:**
- Modify: `ui/src/shell/useGlobalShortcuts.ts`
- Test: `ui/src/shell/useGlobalShortcuts.test.tsx`

**Interfaces:**
- Consumes: Task 2 `split`, `toggleBroadcast`, `focusNeighbour`, `closeGroup` and `activeId`.

- [ ] **Step 1: Write the failing tests**

Read `useGlobalShortcuts.test.tsx` first and follow its helpers for firing keys and seeding stores. Add:

```ts
  it("maps the pane shortcuts", () => {
    const key = (k: string) => shortcutFor({ key: k, ctrlKey: true, shiftKey: true, altKey: false, metaKey: false });
    expect(key("E")).toBe("splitRow");
    expect(key("O")).toBe("splitColumn");
    expect(key("A")).toBe("toggleBroadcast");
    expect(key("W")).toBe("closeTab");
    expect(key("ArrowLeft")).toBe("focusPrev");
    expect(key("ArrowRight")).toBe("focusNext");
  });

  it("runs pane shortcuts in the terminal view only, also from inside a terminal", () => {
    const split = vi.fn();
    const toggleBroadcast = vi.fn();
    const focusNeighbour = vi.fn();
    const closeGroup = vi.fn(async () => undefined);
    useSessionsStore.setState({ activeId: "g", split, toggleBroadcast, focusNeighbour, closeGroup });
    renderHook(() => useGlobalShortcuts());
    const terminal = document.createElement("div");
    terminal.setAttribute("data-terminal", "");
    document.body.append(terminal);
    const press = (key: string) =>
      terminal.dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true }));

    useUiStore.setState({ view: "hosts" });
    press("E");
    expect(split).not.toHaveBeenCalled();

    useUiStore.setState({ view: "terminal" });
    press("E");
    press("O");
    press("A");
    press("ArrowLeft");
    press("ArrowRight");
    press("W");
    expect(split.mock.calls).toEqual([["row"], ["column"]]);
    expect(toggleBroadcast).toHaveBeenCalledOnce();
    expect(focusNeighbour.mock.calls).toEqual([[-1], [1]]);
    expect(closeGroup).toHaveBeenCalledWith("g");
    terminal.remove();
  });
```

Adapt the imports (`renderHook`, `shortcutFor`, `useSessionsStore`, `useUiStore`) to the file's existing style.

- [ ] **Step 2: Run and verify failure**

Run: `bun run test ui/src/shell/useGlobalShortcuts.test.tsx 2>&1 | grep -E "×|Tests"`
Expected: FAIL, because `shortcutFor` returns `null` for these keys.

- [ ] **Step 3: Implement**

`useGlobalShortcuts.ts`:

```ts
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
```

In `shortcutFor`, replace the shift branch with `if (event.shiftKey) return SHIFT_SHORTCUTS[key] ?? null;`. Update
the doc comment to list the new keys.

Replace `TERMINAL_SAFE` and the sidebar-only check:

```ts
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
```

In the handler, replace `if (shortcut === "toggleSidebar" && ui.view !== "terminal") return;` with:

```ts
      if (TERMINAL_VIEW_ONLY.has(shortcut) && ui.view !== "terminal") return;
```

Add these switch cases:

```ts
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
```

- [ ] **Step 4: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -6`
Expected: everything passes. Check the existing shortcut tests, for example the one that `Ctrl+Shift+B` only works in
the terminal view; they should still pass.

- [ ] **Step 5: Commit**

```bash
git add ui/src/shell
git commit -m "feat(ui): pane, broadcast and close-tab shortcuts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Acceptance checklist and full verification

**Files:**
- Create: `docs/acceptance-tp2c.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Write `docs/acceptance-tp2c.md`**

```markdown
# TP2c acceptance: Split panes and broadcast

No DB migration in TP2c.

## Split
- [ ] SSH tab → "Nebeneinander teilen" (Ctrl+Shift+E): a new pane opens on the right with the picker; "Gleicher Host (…)" is preselected; Enter connects (overlay as usual) and the session appears in that pane.
- [ ] The left terminal keeps its scrollback and running program while splitting (e.g. `top` keeps running).
- [ ] "Untereinander teilen" (Ctrl+Shift+O) on the new pane nests a split below it; a 4th pane is possible, then both split buttons are disabled.
- [ ] Picker: search by name/address, arrow keys + Enter, "Lokales Terminal" opens a local shell in the pane, Esc closes the pane.
- [ ] Cancel the connect overlay of a pane: back in the terminal view, the pane shows the picker again.
- [ ] Drag a divider; also focus it (Tab) and use the arrow keys. Terminals refit.
- [ ] Click into a pane / Ctrl+Shift+←/→: the focused pane gets the accent frame and the keyboard.
- [ ] Tab title shows "web-prod-01 · 3"; the sidebar (Sitzung, Design, Snippets) follows the focused pane; a picking pane shows "Kein Terminal".
- [ ] `exit` in one pane: "Sitzung beendet" in that pane only; "Neu verbinden" reconnects in the same pane.
- [ ] × on a pane closes it, the neighbour takes its place; × on the tab / Ctrl+Shift+W closes all its sessions.

## Broadcast
- [ ] Broadcast is disabled with one pane; with two panes Ctrl+Shift+A or the button turns it on: orange frames, "Broadcast an · 2 Bereiche".
- [ ] Typing `hostname` + Enter in the focused pane runs in all connected panes; a picking or ended pane receives nothing.
- [ ] Snippets (insert and ▶) and Ctrl+Shift+V reach all panes while broadcast is on.
- [ ] Closing panes down to one switches broadcast off.
```

- [ ] **Step 2: Update `CLAUDE.md`**

Add this line to the Layout list after the TP2b spec bullet:

```markdown
- `docs/superpowers/specs/2026-10-15-linascp-tp2c-split-broadcast-design.md`: TP2c spec (split panes, broadcast).
```

Change the acceptance bullet to:

```markdown
- `docs/acceptance-tp1.md`: manual acceptance checklist (`docs/acceptance-tp2a.md` … `docs/acceptance-tp2c.md` for
  TP2a–TP2c).
```

- [ ] **Step 3: Full verification**

```bash
export PATH=$HOME/.cargo/bin:$PATH
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings \
  && SHELL=/bin/sh cargo test --workspace 2>&1 | grep -E "test result|FAILED" \
  && bun run typecheck && bun run test 2>&1 | tail -6
```

Expected: all of it is green. Rust is unchanged, so its tests only confirm the state.

- [ ] **Step 4: Commit**

```bash
git add docs/acceptance-tp2c.md CLAUDE.md
git commit -m "docs: TP2c acceptance checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
