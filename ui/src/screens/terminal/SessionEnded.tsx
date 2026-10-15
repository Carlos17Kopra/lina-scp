import { RefreshCw } from "lucide-react";
import { useId } from "react";
import { Button, cx } from "../../components";
import { t } from "../../i18n/de";
import { errorMessage } from "../../ipc/errors";
import { useConnectStore } from "../../stores/connect";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import styles from "./SessionEnded.module.css";

/** Overlay on a pane whose session exited: close it, or reconnect SSH sessions in the same pane. */
export function SessionEnded({ tab }: { tab: Tab }) {
  const closeSession = useSessionsStore((state) => state.closeSession);
  const replacePane = useSessionsStore((state) => state.replacePane);
  const titleId = useId();
  const hostId = tab.kind === "ssh" ? tab.hostId : undefined;

  const reconnect = () => {
    if (!hostId) return;
    const paneId = replacePane(tab.id);
    void useConnectStore.getState().start(hostId, "ssh", { paneId });
  };

  return (
    <div className={styles.overlay}>
      <section className={styles.card} aria-labelledby={titleId}>
        <h2 id={titleId} className={styles.title}>
          {tab.status === "error" ? t.terminal.endedError : t.terminal.ended}
        </h2>
        <p className={styles.text}>{t.terminal.endedText(tab.title)}</p>
        {tab.status === "error" && tab.error ? <p className={cx(styles.error, "mono")}>{errorMessage(tab.error)}</p> : null}
        <div className={styles.actions}>
          <Button onClick={() => void closeSession(tab.id)}>{t.common.close}</Button>
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
