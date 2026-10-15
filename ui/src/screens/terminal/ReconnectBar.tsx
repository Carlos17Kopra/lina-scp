import { useEffect, useState } from "react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { sessionReconnectNow, sessionReconnectStop } from "../../ipc/api";
import { fireAndForget } from "../../stores/connectProgress";
import type { Tab } from "../../stores/sessions";
import styles from "./ReconnectBar.module.css";

/** Strip over a pane whose connection is being restored: countdown, "Jetzt versuchen", "Abbrechen". */
export function ReconnectBar({ tab }: { tab: Tab }) {
  const texts = t.terminal.reconnecting;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const state = tab.reconnect;
  const sessionId = tab.sessionId;
  if (!state || !sessionId) return null;
  const seconds = Math.max(0, Math.ceil((state.retryAt - now) / 1000));
  return (
    <div className={styles.bar}>
      <span className={styles.text}>
        {/* Only the attempt is live: the countdown would be re-announced every second. */}
        <span role="status">{texts.attempt(state.attempt, state.maxAttempts)}</span>
        {seconds > 0 ? texts.countdown(seconds) : texts.connecting}
      </span>
      <Button size="sm" variant="primary" onClick={() => fireAndForget(() => sessionReconnectNow(sessionId))}>
        {texts.now}
      </Button>
      <Button size="sm" variant="ghost" onClick={() => fireAndForget(() => sessionReconnectStop(sessionId))}>
        {texts.stop}
      </Button>
    </div>
  );
}
