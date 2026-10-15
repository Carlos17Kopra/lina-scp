import { AlertTriangle, File, Folder, Link2 } from "lucide-react";
import { type KeyboardEvent, type MouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { cx } from "../../components";
import { t } from "../../i18n/de";
import type { Entry } from "../../ipc/api";
import { type Side, useSftpStore } from "../../stores/sftp";
import { crumbs, formatMode, formatMtime, formatSize, isDirLike, joinPath, parentPath, type SortKey, visibleEntries } from "./entries";
import styles from "./FilePane.module.css";
import { moveCursor, rangeBetween, targets, typeAhead } from "./paneKeys";

export type PaneCommand = "newFolder" | "rename" | "delete" | "deletePermanent";

const COLUMNS: SortKey[] = ["name", "size", "mtime", "mode", "owner"];
const PAGE = 15;
const TYPE_AHEAD_MS = 1000;

function errorText(error: { type: string; kind?: string; message?: string }): string {
  if (error.type === "fs" && error.kind) return t.sftp.fsErrors[error.kind as keyof typeof t.sftp.fsErrors];
  return error.message ?? t.sftp.failed;
}

function icon(entry: Entry) {
  if (entry.kind.type === "symlink") return entry.kind.target === null ? AlertTriangle : Link2;
  return isDirLike(entry) ? Folder : File;
}

export function FilePane({ tabId, side, disabled = false, onCommand }: {
  tabId: string;
  side: Side;
  disabled?: boolean;
  onCommand: (command: PaneCommand, names: string[]) => void;
}) {
  const tab = useSftpStore((s) => s.tabs.find((x) => x.id === tabId));
  const store = useSftpStore.getState;
  const gridRef = useRef<HTMLDivElement>(null);
  const typed = useRef({ prefix: "", at: 0 });
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [draft, setDraft] = useState("");
  const p = tab?.panes[side];
  const rows = useMemo(() => (p ? visibleEntries(p.entries, p.showHidden, p.sort) : []), [p]);
  const names = useMemo(() => [...(p && p.path !== "/" ? [".."] : []), ...rows.map((e) => e.name)], [p, rows]);

  useEffect(() => {
    if (p?.editingPath) setDraft(p.path);
  }, [p?.editingPath, p?.path]);

  if (!tab || !p) return null;
  const label = side === "local" ? t.sftp.local : t.sftp.remote;

  const open = (name: string) => {
    if (name === "..") {
      void store().navigate(tabId, side, parentPath(p.path), p.path.split("/").pop());
      return;
    }
    const entry = p.entries.find((e) => e.name === name);
    if (entry && isDirLike(entry) && !entry.nameLossy) void store().navigate(tabId, side, joinPath(p.path, name));
  };
  const setCursor = (cursor: string | null, extend: boolean) => {
    if (cursor === null) return;
    if (extend) {
      const anchor = p.anchor ?? p.cursor ?? cursor;
      store().select(tabId, side, { selection: rangeBetween(names, anchor, cursor), cursor, anchor });
    } else {
      store().select(tabId, side, { selection: p.selection, cursor, anchor: cursor });
    }
  };
  const toggle = (name: string) => {
    if (name === "..") return;
    const selection = p.selection.includes(name) ? p.selection.filter((n) => n !== name) : [...p.selection, name];
    store().select(tabId, side, { selection, cursor: name, anchor: name });
  };
  const command = (c: PaneCommand) => {
    const chosen = targets(p);
    if (c === "newFolder") onCommand(c, []);
    else if (c === "rename" ? chosen.length === 1 : chosen.length > 0) onCommand(c, chosen);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled || event.target !== event.currentTarget) return;
    const { key, ctrlKey, shiftKey, altKey } = event;
    const handled = () => event.preventDefault();
    if (["ArrowDown", "ArrowUp", "Home", "End", "PageDown", "PageUp"].includes(key) && !ctrlKey) {
      handled();
      return setCursor(moveCursor(names, p.cursor, key, PAGE), shiftKey);
    }
    if (ctrlKey && !altKey) {
      const k = key.toLowerCase();
      if (k === "a") store().select(tabId, side, { selection: rows.filter((e) => !e.nameLossy).map((e) => e.name), cursor: p.cursor, anchor: p.anchor });
      else if (k === "h") store().toggleHidden(tabId, side);
      else if (k === "r") void store().reload(tabId, side);
      else if (k === "l") store().setEditingPath(tabId, side, true);
      else return;
      handled();
      return;
    }
    const action: Record<string, () => void> = {
      Tab: () => {
        const other: Side = side === "local" ? "remote" : "local";
        store().setActivePane(tabId, other);
        document.querySelector<HTMLElement>(`[data-sftp-grid="${other}"]`)?.focus();
      },
      Enter: () => p.cursor && open(p.cursor),
      Backspace: () => open(".."),
      " ": () => p.cursor && toggle(p.cursor),
      F2: () => command("rename"),
      F7: () => command("newFolder"),
      F8: () => command("delete"),
      Delete: () => command(shiftKey && side === "local" ? "deletePermanent" : "delete"),
    };
    const run = action[key];
    if (run) {
      handled();
      run();
      return;
    }
    if (key.length === 1 && !altKey && key !== " ") {
      const now = Date.now();
      typed.current = { prefix: now - typed.current.at < TYPE_AHEAD_MS ? typed.current.prefix + key : key, at: now };
      const hit = typeAhead(names, typed.current.prefix, p.cursor);
      if (hit) setCursor(hit, false);
    }
  };

  const onRowClick = (event: MouseEvent, name: string) => {
    gridRef.current?.focus();
    store().setActivePane(tabId, side);
    if (event.ctrlKey) toggle(name);
    else if (event.shiftKey && p.anchor) setCursor(name, true);
    else store().select(tabId, side, { selection: name === ".." ? [] : [name], cursor: name, anchor: name });
  };
  const onContextMenu = (event: MouseEvent, name: string | null) => {
    event.preventDefault();
    if (name && name !== ".." && !p.selection.includes(name)) store().select(tabId, side, { selection: [name], cursor: name, anchor: name });
    setMenu({ x: event.clientX, y: event.clientY });
  };
  const menuItem = (text: string, run: () => void) => (
    <button type="button" role="menuitem" className={styles.menuItem} onClick={() => { setMenu(null); run(); }}>
      {text}
    </button>
  );

  return (
    <section className={cx(styles.pane, disabled && styles.dim, tab.active === side && styles.active)} aria-label={label}>
      <header className={styles.head}>
        <span className={styles.side}>{label}</span>
        {p.editingPath ? (
          <input
            aria-label={t.sftp.path}
            className={styles.pathField}
            value={draft}
            autoFocus
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter" && !e.nativeEvent.isComposing) void store().navigate(tabId, side, draft.trim() || "/");
              if (e.key === "Escape") store().setEditingPath(tabId, side, false);
            }}
            onBlur={() => store().setEditingPath(tabId, side, false)}
          />
        ) : (
          <nav className={styles.crumbs} aria-label={t.sftp.path} onClick={(e) => e.target === e.currentTarget && store().setEditingPath(tabId, side, true)}>
            {crumbs(p.path).map((c) => (
              <button key={c.path} type="button" className={styles.crumb} onClick={() => void store().navigate(tabId, side, c.path)}>
                {c.label}
              </button>
            ))}
          </nav>
        )}
      </header>
      {p.error ? (
        <details role="alert" className={styles.error}>
          <summary>{errorText(p.error)}</summary>
          <code>{"message" in p.error ? p.error.message : p.error.type}</code>
        </details>
      ) : null}
      <div
        ref={gridRef}
        role="grid"
        data-sftp-grid={side}
        aria-label={label}
        aria-multiselectable
        aria-busy={p.loading}
        tabIndex={0}
        className={styles.grid}
        onKeyDown={onKeyDown}
        onFocus={() => store().setActivePane(tabId, side)}
        onContextMenu={(e) => onContextMenu(e, null)}
      >
        <div role="row" className={styles.header}>
          {COLUMNS.map((key) => (
            <div
              key={key}
              role="columnheader"
              aria-sort={p.sort.key === key ? (p.sort.dir === "asc" ? "ascending" : "descending") : "none"}
              className={styles.colHead}
              onClick={() => store().setSort(tabId, side, key)}
            >
              {t.sftp.columns[key]}
            </div>
          ))}
        </div>
        {names.map((name) => {
          const entry = rows.find((e) => e.name === name);
          const Icon = entry ? icon(entry) : Folder;
          return (
            <div
              key={name}
              role="row"
              aria-selected={p.selection.includes(name)}
              title={entry?.nameLossy ? t.sftp.lossyName : entry?.kind.type === "symlink" && entry.kind.target === null ? t.sftp.brokenLink : undefined}
              className={cx(styles.row, p.cursor === name && styles.cursor, entry?.nameLossy && styles.lossy)}
              onClick={(e) => onRowClick(e, name)}
              onDoubleClick={() => open(name)}
              onContextMenu={(e) => { e.stopPropagation(); onContextMenu(e, name); }}
            >
              <div role="gridcell" className={styles.name}>
                <Icon size={14} aria-hidden />
                <span>{name}</span>
              </div>
              <div role="gridcell" className="mono">{entry && !isDirLike(entry) ? formatSize(entry.size) : ""}</div>
              <div role="gridcell" className="mono">{entry ? formatMtime(entry.mtime) : ""}</div>
              <div role="gridcell" className="mono">{entry ? formatMode(entry.mode) : ""}</div>
              <div role="gridcell">{entry?.owner ?? ""}</div>
            </div>
          );
        })}
      </div>
      {menu ? (
        <div
          role="menu"
          className={styles.menu}
          style={{ left: menu.x, top: menu.y }}
          onMouseLeave={() => setMenu(null)}
          onKeyDown={(e) => e.key === "Escape" && setMenu(null)}
        >
          {menuItem(t.sftp.open, () => p.cursor && open(p.cursor))}
          {menuItem(t.sftp.rename, () => command("rename"))}
          {menuItem(t.sftp.delete, () => command("delete"))}
          {menuItem(t.sftp.newFolder, () => command("newFolder"))}
          {menuItem(t.sftp.reload, () => void store().reload(tabId, side))}
        </div>
      ) : null}
    </section>
  );
}
