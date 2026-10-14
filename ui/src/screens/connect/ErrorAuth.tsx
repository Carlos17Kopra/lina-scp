import { Lock } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import { Button, Checkbox, cx, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Host } from "../../ipc/api";
import { hostAuthConfig, hostProfile, methodLabel } from "../../model/hostFormat";
import { type ConnectFlow, MAX_ATTEMPTS, useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import styles from "./Connect.module.css";

function methodStatus(method: string, attempt: number): { text: string; red: boolean } {
  const texts = t.connect.auth;
  if (method === "password") return { text: texts.passwordRejected(Math.min(attempt, MAX_ATTEMPTS), MAX_ATTEMPTS), red: true };
  if (method === "publickey") return { text: texts.noMatchingKey, red: false };
  return { text: texts.rejected, red: true };
}

export function ErrorAuth({
  flow,
  host,
  error,
  onEdit,
}: {
  flow: ConnectFlow;
  host: Host;
  error: CoreError;
  /** Opens the login profile (if used) or the host in the editor. */
  onEdit: (target: { kind: "host" | "profile"; id: string }) => void;
}) {
  const profiles = useHostsStore((state) => state.profiles);
  const retry = useConnectStore((state) => state.retry);
  const retryWithPassword = useConnectStore((state) => state.retryWithPassword);
  const cancel = useConnectStore((state) => state.cancel);
  const [password, setPassword] = useState("");
  const [save, setSave] = useState(true);
  const [missing, setMissing] = useState(false);
  const titleId = useId();
  const formId = useId();
  const texts = t.connect.auth;

  const profile = hostProfile(host, profiles);
  const auth = hostAuthConfig(host, profiles);
  const askPassword = auth?.method === "password";
  const rejected = error.type === "auth" ? error.rejectedMethods : [];

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    if (flow.submitting) return;
    if (!askPassword) {
      void retry();
      return;
    }
    if (!password) {
      setMissing(true);
      return;
    }
    void retryWithPassword(password, save);
  };

  return (
    <section className={cx(styles.card, styles.cardNarrow)} aria-labelledby={titleId}>
      <span className={cx(styles.icon, styles.iconRed)} aria-hidden="true">
        <Lock size={24} />
      </span>
      <h1 id={titleId} className={styles.cardTitle}>
        {texts.title}
      </h1>
      <p className={styles.text}>
        {texts.textBefore}
        <code className={styles.code}>{host.address}</code>
        {profile ? texts.rejectedProfile(profile.name) : texts.rejectedHost}
      </p>
      {rejected.length > 0 ? (
        <ul className={styles.methods} aria-label={texts.methods}>
          {rejected.map((method) => {
            const status = methodStatus(method, flow.attempt);
            return (
              <li key={method}>
                <span className="mono">{method}</span>
                <span className={status.red ? styles.methodRed : styles.methodMuted}>{status.text}</span>
              </li>
            );
          })}
        </ul>
      ) : null}
      {askPassword ? (
        <form id={formId} className={styles.form} onSubmit={submit} noValidate>
          <TextField label={texts.user} mono readOnly value={host.user ?? profile?.user ?? ""} onChange={() => undefined} />
          <TextField
            type="password"
            label={texts.password}
            placeholder={texts.passwordPlaceholder}
            autoFocus
            autoComplete="off"
            value={password}
            error={missing && !password ? texts.passwordRequired : null}
            onChange={(value) => {
              setPassword(value);
              setMissing(false);
            }}
          />
          <Checkbox label={profile ? texts.saveInProfile(profile.name) : texts.saveForHost} checked={save} onChange={setSave} />
        </form>
      ) : (
        <p className={styles.text}>{texts.otherMethod(auth ? methodLabel(auth.method) : t.hosts.none)}</p>
      )}
      <div className={styles.footer}>
        <Button variant="ghost" onClick={() => void cancel()}>
          {t.common.cancel}
        </Button>
        <Button onClick={() => onEdit(profile ? { kind: "profile", id: profile.id } : { kind: "host", id: host.id })}>
          {profile ? texts.editProfile : t.hosts.edit}
        </Button>
        <span className={styles.spacer} />
        {askPassword ? (
          <Button variant="primary" type="submit" form={formId} disabled={flow.submitting}>
            {flow.submitting ? t.connect.error.connecting : texts.retry}
          </Button>
        ) : (
          <Button variant="primary" disabled={flow.submitting} onClick={() => submit()}>
            {flow.submitting ? t.connect.error.connecting : texts.retry}
          </Button>
        )}
      </div>
    </section>
  );
}
