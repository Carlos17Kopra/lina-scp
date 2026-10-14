import { LoaderCircle } from "lucide-react";
import type { Tone } from "../../components";
import { t } from "../../i18n/de";
import styles from "./Connect.module.css";
import type { ConnectView } from "./connectText";

const STATUS: Record<ConnectView, { tone: Tone; suffix?: string }> = {
  progress: { tone: "accent" },
  hostKeyNew: { tone: "amber", suffix: t.connect.tabStatus.waiting },
  hostKeyChanged: { tone: "red", suffix: t.connect.tabStatus.blocked },
  errorConnection: { tone: "red", suffix: t.connect.tabStatus.disconnected },
  errorAuth: { tone: "red", suffix: t.connect.tabStatus.notLoggedIn },
};

/** Single static tab reflecting the connection state ("nas · getrennt"). */
export function ConnectTabBar({ name, view, busy }: { name: string; view: ConnectView; busy: boolean }) {
  const status = STATUS[view];
  const spinning = view === "progress" || busy;
  return (
    <div className={styles.tabBar} role="tablist" aria-label={t.connect.sessions}>
      <span role="tab" aria-selected="true" className={styles.tab} data-tone={spinning ? "accent" : status.tone}>
        {spinning ? (
          <LoaderCircle size={14} className={styles.spin} aria-hidden="true" />
        ) : (
          <span className={styles.dot} aria-hidden="true" />
        )}
        <span>{status.suffix && !busy ? `${name} · ${status.suffix}` : name}</span>
      </span>
    </div>
  );
}
