import { RefreshCw, RotateCw } from "lucide-react";
import { useId } from "react";
import { Button, cx } from "../../components";
import { t } from "../../i18n/de";
import { type CoreError, sessionReconnectNow } from "../../ipc/api";
import { errorMessage } from "../../ipc/errors";
import { useConnectStore } from "../../stores/connect";
import { fireAndForget } from "../../stores/connectProgress";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import styles from "./SessionEnded.module.css";

/** Reasons a silent retry cannot fix (mirrors core `is_fatal`). */
const FATAL_LOST: ReadonlySet<CoreError["type"]> = new Set(["inputRequired", "config", "auth"]);

function lostReason(error: CoreError | undefined): string | null {
  if (!error) return null;
  if (error.type === "inputRequired") return t.terminal.reconnecting.inputRequired;
  if (error.type === "cancelled") return t.terminal.reconnecting.cancelled;
  return errorMessage(error);
}

/**
 * Overlay on a pane whose session ended or whose connection was lost: close it, retry silently
 * (history kept), or reconnect SSH sessions interactively in the same pane.
 */
export function SessionEnded({ tab }: { tab: Tab }) {
  const closeSession = useSessionsStore((state) => state.closeSession);
  const replacePane = useSessionsStore((state) => state.replacePane);
  const titleId = useId();
  const lost = tab.status === "lost";
  const hostGone = lost && tab.error?.type === "config";
  const hostId = tab.kind === "ssh" && !hostGone ? tab.hostId : undefined;
  const canRetry = lost && !!tab.sessionId && !(tab.error && FATAL_LOST.has(tab.error.type));

  const reconnect = () => {
    if (!hostId) return;
    const paneId = replacePane(tab.id);
    void useConnectStore.getState().start(hostId, "ssh", { paneId });
  };
  const retry = () => {
    const sessionId = tab.sessionId;
    if (sessionId) fireAndForget(() => sessionReconnectNow(sessionId));
  };

  const title = lost ? t.terminal.reconnecting.lost : tab.status === "error" ? t.terminal.endedError : t.terminal.ended;
  const detail = lost ? lostReason(tab.error) : tab.status === "error" && tab.error ? errorMessage(tab.error) : null;

  return (
    <div className={styles.overlay}>
      <section className={styles.card} aria-labelledby={titleId}>
        <h2 id={titleId} className={styles.title}>
          {title}
        </h2>
        <p className={styles.text}>{lost ? t.terminal.reconnecting.lostText(tab.title) : t.terminal.endedText(tab.title)}</p>
        {detail ? <p className={cx(styles.error, "mono")}>{detail}</p> : null}
        <div className={styles.actions}>
          <Button onClick={() => void closeSession(tab.id)}>{t.common.close}</Button>
          {canRetry ? (
            <Button icon={RotateCw} onClick={retry}>
              {t.terminal.reconnecting.retry}
            </Button>
          ) : null}
          {hostId ? (
            <Button variant="primary" icon={RefreshCw} onClick={reconnect}>
              {t.terminal.reconnect}
            </Button>
          ) : null}
        </div>
      </section>
    </div>
  );
}
