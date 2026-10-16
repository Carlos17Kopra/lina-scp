import { Bookmark as BookmarkIcon, X } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { t } from "../../i18n/de";
import { type BookmarkKey, useBookmarksStore } from "../../stores/bookmarks";
import styles from "./FilePane.module.css";

const labelOf = (path: string) => (path === "/" ? "/" : path.split("/").filter(Boolean).pop() ?? path);

/** The bookmark button of a pane header with its menu. */
export function BookmarkMenu({ bookmarkKey, path, onNavigate }: { bookmarkKey: BookmarkKey; path: string; onNavigate: (path: string) => void }) {
  const list = useBookmarksStore((s) => s.byKey[bookmarkKey]);
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const bookmarks = list ?? [];
  const marked = bookmarks.some((b) => b.path === path);
  const store = useBookmarksStore.getState;

  useEffect(() => {
    if (open) menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [open]);

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) buttonRef.current?.focus();
  };
  const toggleOpen = () => {
    if (open) return close(false);
    setOpen(true);
    void store().load(bookmarkKey).catch(() => undefined);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      return close(true);
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const items = [...(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const next = event.key === "ArrowDown" ? Math.min(at + 1, items.length - 1) : Math.max(at - 1, 0);
    items[next]?.focus();
  };

  return (
    <span className={styles.bookmarks}>
      <button
        ref={buttonRef}
        type="button"
        className={styles.headButton}
        aria-label={t.sftp.bookmarks.button}
        aria-pressed={marked}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={toggleOpen}
      >
        <BookmarkIcon size={15} fill={marked ? "currentColor" : "none"} />
      </button>
      {open ? (
        <div
          ref={menuRef}
          role="menu"
          className={styles.bookmarkMenu}
          onKeyDown={onKeyDown}
          onBlur={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && e.relatedTarget !== buttonRef.current && close(false)}
        >
          <button
            type="button"
            role="menuitem"
            className={styles.menuItem}
            onClick={() => void store().toggle(bookmarkKey, path)}
          >
            {marked ? t.sftp.bookmarks.remove : t.sftp.bookmarks.add}
          </button>
          <hr className={styles.menuRule} />
          {bookmarks.length === 0 ? <span className={styles.menuEmpty}>{t.sftp.bookmarks.empty}</span> : null}
          {bookmarks.map((b) => (
            <span key={b.id} className={styles.bookmarkRow}>
              <button
                type="button"
                role="menuitem"
                title={b.path}
                className={styles.menuItem}
                onClick={() => {
                  close(false);
                  onNavigate(b.path);
                }}
              >
                {labelOf(b.path)}
              </button>
              <button
                type="button"
                className={styles.headButton}
                aria-label={t.sftp.bookmarks.removeOne(labelOf(b.path))}
                onClick={() => void store().remove(bookmarkKey, b.id)}
              >
                <X size={13} />
              </button>
            </span>
          ))}
        </div>
      ) : null}
    </span>
  );
}
