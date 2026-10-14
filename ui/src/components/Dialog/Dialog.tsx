import { X } from "lucide-react";
import { type KeyboardEvent, type ReactNode, type RefObject, useEffect, useId, useRef } from "react";
import { t } from "../../i18n/de";
import { cx } from "../cx";
import { IconButton } from "../IconButton/IconButton";
import styles from "./Dialog.module.css";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE));
}

export interface DialogProps {
  title: string;
  open: boolean;
  onClose: () => void;
  size?: "sm" | "md" | "lg";
  children: ReactNode;
  footer?: ReactNode;
  subtitle?: ReactNode;
  /** Leading visual in the header, e.g. an initials chip. */
  icon?: ReactNode;
  /** Use "alertdialog" for destructive/blocking confirmations. */
  role?: "dialog" | "alertdialog";
  initialFocusRef?: RefObject<HTMLElement | null>;
  hideClose?: boolean;
  className?: string;
}

export function Dialog({
  title,
  open,
  onClose,
  size = "md",
  children,
  footer,
  subtitle,
  icon,
  role = "dialog",
  initialFocusRef,
  hideClose = false,
  className,
}: DialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return undefined;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = panelRef.current;
    const body = bodyRef.current;
    const target =
      initialFocusRef?.current ?? (body ? focusables(body)[0] : undefined) ?? (panel ? focusables(panel)[0] : undefined) ?? panel;
    target?.focus();
    return () => previous?.focus();
  }, [open, initialFocusRef]);

  // Keep focus inside the panel: pull it back when it lands outside any modal (e.g. on <body> or the page behind).
  useEffect(() => {
    if (!open) return undefined;
    const onFocusIn = (event: FocusEvent) => {
      const panel = panelRef.current;
      const target = event.target;
      if (!panel || !(target instanceof Element)) return;
      if (panel.contains(target) || target.closest('[aria-modal="true"]')) return;
      (focusables(panel)[0] ?? panel).focus();
    };
    document.addEventListener("focusin", onFocusIn);
    return () => document.removeEventListener("focusin", onFocusIn);
  }, [open]);

  if (!open) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const panel = panelRef.current;
    if (!panel) return;
    const items = focusables(panel);
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) {
      event.preventDefault();
      panel.focus();
      return;
    }
    const active = document.activeElement;
    if (event.shiftKey && (active === first || active === panel)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className={styles.backdrop}
      onMouseDown={(event) => {
        // A click on the backdrop padding must not blur the panel.
        if (event.target === event.currentTarget) event.preventDefault();
      }}
    >
      <div
        ref={panelRef}
        role={role}
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cx(styles.panel, styles[size], className)}
        onKeyDown={onKeyDown}
      >
        <header className={styles.header}>
          {icon}
          <div className={styles.titles}>
            <h2 id={titleId} className={styles.title}>
              {title}
            </h2>
            {subtitle ? <p className={styles.subtitle}>{subtitle}</p> : null}
          </div>
          {hideClose ? null : <IconButton label={t.common.close} icon={X} size="sm" onClick={onClose} />}
        </header>
        <div ref={bodyRef} className={styles.body}>
          {children}
        </div>
        {footer ? <footer className={styles.footer}>{footer}</footer> : null}
      </div>
    </div>
  );
}
