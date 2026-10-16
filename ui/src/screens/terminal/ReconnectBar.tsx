import { useEffect, useState } from "react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import styles from "./ReconnectBar.module.css";

/** Strip over a pane whose connection is being restored: countdown, "Jetzt versuchen", "Abbrechen". */
export function ReconnectBar({ state, onNow, onStop }: {
  state: { attempt: number; maxAttempts: number; retryAt: number };
  onNow: () => void;
  onStop: () => void;
}) {
  const texts = t.terminal.reconnecting;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const seconds = Math.max(0, Math.ceil((state.retryAt - now) / 1000));
  return (
    <div className={styles.bar}>
      <span className={styles.text}>
        {/* Only the attempt is live: the countdown would be re-announced every second. */}
        <span role="status">{texts.attempt(state.attempt, state.maxAttempts)}</span>
        {seconds > 0 ? texts.countdown(seconds) : texts.connecting}
      </span>
      <Button size="sm" variant="primary" onClick={onNow}>{texts.now}</Button>
      <Button size="sm" variant="ghost" onClick={onStop}>{texts.stop}</Button>
    </div>
  );
}
