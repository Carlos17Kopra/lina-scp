import { TriangleAlert } from "lucide-react";
import { useId, useState } from "react";
import { Button, Checkbox, cx } from "../../components";
import { t } from "../../i18n/de";
import type { HostKeyDecision, HostKeyInfo } from "../../ipc/api";
import { formatEndpoint } from "../../model/hostFormat";
import styles from "./Connect.module.css";
import { algorithmLabel, openSshWarning } from "./connectText";

/**
 * Changed host key: the safe "Verbindung abbrechen" is the visually strongest (inverted) button and
 * focused; "Schlüssel ersetzen und verbinden" stays disabled until the confirmation is ticked.
 */
export function HostKeyChangedDialog({
  host,
  port,
  stored,
  current,
  onDecision,
  onEditHost,
}: {
  host: string;
  port: number;
  stored: HostKeyInfo;
  current: HostKeyInfo;
  onDecision: (decision: HostKeyDecision) => void;
  onEditHost: () => void;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const titleId = useId();
  const textId = useId();
  const texts = t.connect.hostKeyChanged;

  return (
    <section role="alertdialog" aria-labelledby={titleId} aria-describedby={textId} className={cx(styles.card, styles.cardDanger)}>
      <span className={cx(styles.icon, styles.iconRed)} aria-hidden="true">
        <TriangleAlert size={24} />
      </span>
      <h1 id={titleId} className={cx(styles.cardTitle, styles.dangerTitle)}>
        {texts.title}
      </h1>
      <p id={textId} className={styles.text}>
        {texts.textBefore}
        <code className={styles.code}>{formatEndpoint(null, host, port)}</code>
        {texts.textAfter}
      </p>
      <div className={styles.compare}>
        <div className={styles.keyBox}>
          <span className={styles.caption}>{texts.stored(algorithmLabel(stored.algorithm))}</span>
          <code className={styles.keyValue}>{stored.sha256}</code>
        </div>
        <div className={cx(styles.keyBox, styles.keyBoxNew)}>
          <span className={cx(styles.caption, styles.captionRed)}>{texts.current(algorithmLabel(current.algorithm))}</span>
          <code className={styles.keyValue}>{current.sha256}</code>
        </div>
      </div>
      <details className={styles.details}>
        <summary>{texts.technical}</summary>
        <pre className={styles.pre}>{openSshWarning(stored.algorithm)}</pre>
      </details>
      <Checkbox label={texts.confirm} checked={confirmed} onChange={setConfirmed} />
      <div className={styles.footer}>
        <Button variant="danger" disabled={!confirmed} onClick={() => onDecision("replace")}>
          {texts.replace}
        </Button>
        <span className={styles.spacer} />
        <Button variant="ghost" onClick={onEditHost}>
          {t.hosts.edit}
        </Button>
        <Button variant="inverted" autoFocus onClick={() => onDecision("reject")}>
          {texts.abort}
        </Button>
      </div>
    </section>
  );
}
