import { Pencil, RefreshCw, WifiOff } from "lucide-react";
import { useId } from "react";
import { Button, Checkbox, cx } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Host } from "../../ipc/api";
import { formatEndpoint, hostProfile } from "../../model/hostFormat";
import { type ConnectFlow, MAX_ATTEMPTS, useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import styles from "./Connect.module.css";
import { connectionErrorText } from "./connectText";

export function ErrorConnection({
  flow,
  host,
  error,
  onEditHost,
}: {
  flow: ConnectFlow;
  host: Host;
  error: CoreError;
  onEditHost: () => void;
}) {
  const profiles = useHostsStore((state) => state.profiles);
  const retry = useConnectStore((state) => state.retry);
  const cancel = useConnectStore((state) => state.cancel);
  const setAutoRetry = useConnectStore((state) => state.setAutoRetry);
  const titleId = useId();
  const causesId = useId();
  const busy = flow.retrying;
  const text = connectionErrorText(error, host);
  const endpoint = formatEndpoint(host.user ?? hostProfile(host, profiles)?.user, host.address, host.port, true);

  return (
    <section className={styles.page} aria-labelledby={titleId}>
      <span className={cx(styles.icon, styles.iconRed, styles.iconLarge)} aria-hidden="true">
        {busy ? <RefreshCw size={26} className={styles.spin} /> : <WifiOff size={26} />}
      </span>
      <h1 id={titleId} className={styles.cardTitle}>
        {busy ? t.connect.error.retrying : text.title}
      </h1>
      <p className={styles.subtitle}>{`${endpoint} · ${t.connect.attempt(Math.min(flow.attempt, MAX_ATTEMPTS), MAX_ATTEMPTS)}`}</p>
      <p className={styles.text}>{text.message}</p>
      {text.causes.length > 0 ? (
        <div>
          <h2 id={causesId} className={styles.label}>
            {t.connect.error.causes}
          </h2>
          <ul className={styles.causes} aria-labelledby={causesId}>
            {text.causes.map((cause) => (
              <li key={cause}>{cause}</li>
            ))}
          </ul>
        </div>
      ) : null}
      {text.detail ? <pre className={styles.errorPre}>{text.detail}</pre> : null}
      <div className={styles.actions}>
        <Button variant="primary" icon={RefreshCw} disabled={busy} onClick={() => void retry()}>
          {busy ? t.connect.error.connecting : t.common.retry}
        </Button>
        <Button icon={Pencil} onClick={onEditHost}>
          {t.hosts.edit}
        </Button>
        <Button variant="ghost" onClick={() => void cancel()}>
          {t.connect.error.toOverview}
        </Button>
        <span className={styles.spacer} />
        <Checkbox label={t.connect.error.autoRetry} checked={flow.autoRetry} onChange={setAutoRetry} />
      </div>
    </section>
  );
}
