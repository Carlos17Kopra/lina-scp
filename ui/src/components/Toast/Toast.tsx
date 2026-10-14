import { X } from "lucide-react";
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { t } from "../../i18n/de";
import { Button } from "../Button/Button";
import { cx } from "../cx";
import { IconButton } from "../IconButton/IconButton";
import type { Tone } from "../tone";
import styles from "./Toast.module.css";

export interface ToastOptions {
  tone: Tone;
  message: string;
  /** Technical details; adds a "Details kopieren" action. */
  details?: string;
  durationMs?: number;
}

export interface ToastApi {
  show(options: ToastOptions): number;
  dismiss(id: number): void;
}

interface ToastEntry extends ToastOptions {
  id: number;
}

const DEFAULT_DURATION_MS = 5000;
const ERROR_DURATION_MS = 10000;

const ToastContext = createContext<ToastApi | null>(null);

function ToastItem({ entry, onDismiss }: { entry: ToastEntry; onDismiss: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    if (!entry.details) return;
    navigator.clipboard?.writeText(entry.details).then(
      () => setCopied(true),
      () => setCopied(false),
    );
  };
  return (
    <div role={entry.tone === "red" ? "alert" : "status"} className={cx(styles.toast, styles[entry.tone])}>
      <p className={styles.message}>{entry.message}</p>
      {entry.details ? (
        <Button size="sm" variant="ghost" onClick={copy}>
          {copied ? t.common.copied : t.toast.copyDetails}
        </Button>
      ) : null}
      <IconButton label={t.toast.dismiss} icon={X} size="sm" onClick={onDismiss} />
    </div>
  );
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [entries, setEntries] = useState<ToastEntry[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    setEntries((current) => current.filter((entry) => entry.id !== id));
    const timer = timers.current.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.current.delete(id);
    }
  }, []);

  const show = useCallback(
    (options: ToastOptions) => {
      const id = nextId.current;
      nextId.current += 1;
      setEntries((current) => [...current, { ...options, id }]);
      const duration = options.durationMs ?? (options.tone === "red" ? ERROR_DURATION_MS : DEFAULT_DURATION_MS);
      timers.current.set(
        id,
        setTimeout(() => dismiss(id), duration),
      );
      return id;
    },
    [dismiss],
  );

  useEffect(() => {
    const pending = timers.current;
    return () => {
      pending.forEach((timer) => clearTimeout(timer));
      pending.clear();
    };
  }, []);

  const api = useMemo<ToastApi>(() => ({ show, dismiss }), [show, dismiss]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className={styles.region} role="region" aria-label={t.toast.region}>
        {entries.map((entry) => (
          <ToastItem key={entry.id} entry={entry} onDismiss={() => dismiss(entry.id)} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) {
    throw new Error("useToast must be used inside <ToastProvider>");
  }
  return api;
}
