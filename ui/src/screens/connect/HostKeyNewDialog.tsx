import { Copy, ShieldAlert } from "lucide-react";
import { useId, useState } from "react";
import { Button, cx, Segmented } from "../../components";
import { t } from "../../i18n/de";
import type { HostKeyDecision, HostKeyInfo } from "../../ipc/api";
import styles from "./Connect.module.css";
import { algorithmLabel } from "./connectText";

type Format = "sha256" | "md5";

const FORMAT_OPTIONS: ReadonlyArray<{ value: Format; label: string }> = [
  { value: "sha256", label: "SHA256" },
  { value: "md5", label: "MD5" },
];

export function HostKeyNewDialog({
  host,
  port,
  info,
  onDecision,
}: {
  host: string;
  port: number;
  info: HostKeyInfo;
  onDecision: (decision: HostKeyDecision) => void;
}) {
  const [format, setFormat] = useState<Format>("sha256");
  const [copied, setCopied] = useState(false);
  const titleId = useId();
  const fingerprint = format === "sha256" ? info.sha256 : info.md5;
  const texts = t.connect.hostKeyNew;

  const copy = () => {
    navigator.clipboard?.writeText(fingerprint).then(
      () => setCopied(true),
      () => setCopied(false),
    );
  };

  return (
    <section className={styles.card} aria-labelledby={titleId}>
      <span className={cx(styles.icon, styles.iconAmber)} aria-hidden="true">
        <ShieldAlert size={24} />
      </span>
      <h1 id={titleId} className={styles.cardTitle}>
        {texts.title}
      </h1>
      <p className={styles.text}>
        {texts.textBefore}
        <code className={styles.code}>{`${host}:${port}`}</code>
        {texts.textAfter}
      </p>
      <div className={styles.fingerprint}>
        <div className={styles.fingerprintHead}>
          <span>{texts.caption(algorithmLabel(info.algorithm), format === "sha256" ? "SHA256" : "MD5")}</span>
          <Button size="sm" variant="ghost" icon={Copy} onClick={copy}>
            {copied ? t.common.copied : t.common.copy}
          </Button>
        </div>
        <code className={styles.fingerprintValue}>{fingerprint}</code>
        <Segmented
          size="sm"
          label={texts.format}
          value={format}
          options={FORMAT_OPTIONS}
          onChange={(value) => {
            setFormat(value);
            setCopied(false);
          }}
        />
      </div>
      <div className={styles.footer}>
        <Button variant="ghost" onClick={() => onDecision("reject")}>
          {t.common.cancel}
        </Button>
        <span className={styles.spacer} />
        <Button onClick={() => onDecision("acceptOnce")}>{texts.once}</Button>
        <Button variant="primary" onClick={() => onDecision("trust")}>
          {texts.trust}
        </Button>
      </div>
    </section>
  );
}
