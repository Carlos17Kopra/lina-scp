import { useId, useState } from "react";
import { Button, ProgressBar, StepList } from "../../components";
import { t } from "../../i18n/de";
import type { Host, Step } from "../../ipc/api";
import { formatEndpoint, hostAuthConfig, hostColorVar, hostProfile, initials, methodLabel } from "../../model/hostFormat";
import { type ConnectFlow, useConnectStore } from "../../stores/connect";
import { progressRatio } from "../../stores/connectProgress";
import { useHostsStore } from "../../stores/hosts";
import styles from "./Connect.module.css";
import { isInlinePrompt, PromptForm } from "./PromptForm";
import { stepItems } from "./steps";

export function ConnectProgress({ flow, host }: { flow: ConnectFlow; host: Host }) {
  const hosts = useHostsStore((state) => state.hosts);
  const profiles = useHostsStore((state) => state.profiles);
  const cancel = useConnectStore((state) => state.cancel);
  const answer = useConnectStore((state) => state.answer);
  const [showLog, setShowLog] = useState(false);
  const logId = useId();

  const jump = host.jumpHostId ? hosts.find((h) => h.id === host.jumpHostId) : undefined;
  const profile = hostProfile(host, profiles);
  const auth = hostAuthConfig(host, profiles);
  const endpoint = formatEndpoint(host.user ?? profile?.user, host.address, host.port, true);
  const meta: Partial<Record<Step, string>> = {
    jumpHost: jump ? formatEndpoint(jump.user, jump.address, jump.port, true) : undefined,
    tcp: formatEndpoint(null, host.address, host.port, true),
    auth: profile ? t.connect.authProfile(profile.name, methodLabel(profile.auth.method)) : auth ? methodLabel(auth.method) : undefined,
    channel: "session",
    shell: t.connect.shellMeta(flow.cols, flow.rows),
  };
  const pending = flow.prompt;
  const detail =
    pending && isInlinePrompt(pending.prompt) ? (
      <PromptForm key={pending.promptId} prompt={pending.prompt} onAnswer={(value) => void answer(pending.promptId, value)} />
    ) : null;

  return (
    <div className={styles.column}>
      <div className={styles.header}>
        <span className={styles.pulse} aria-hidden="true">
          <span className={styles.avatar} style={{ color: hostColorVar(host.color) }}>
            {initials(host.name)}
          </span>
        </span>
        <div>
          <h1 className={styles.title}>{t.connect.connecting(host.name)}</h1>
          <p className={styles.subtitle}>{jump ? t.connect.via(endpoint, jump.name) : endpoint}</p>
        </div>
      </div>
      <ProgressBar value={progressRatio(flow)} tone={flow.protocol === "sftp" ? "amber" : "accent"} label={t.connect.progress} />
      <StepList steps={stepItems(flow, { jumpName: jump?.name, meta, detail })} />
      {detail && !flow.current ? detail : null}
      <div>
        <button
          type="button"
          className={styles.logToggle}
          aria-expanded={showLog}
          aria-controls={logId}
          onClick={() => setShowLog((value) => !value)}
        >
          {showLog ? t.connect.hideLog : t.connect.showLog}
        </button>
        {showLog ? (
          <pre id={logId} className={styles.log}>
            {flow.log.length > 0 ? flow.log.join("\n") : t.connect.emptyLog}
          </pre>
        ) : null}
      </div>
      <div className={styles.actions}>
        <Button onClick={() => void cancel()}>{t.common.cancel}</Button>
      </div>
    </div>
  );
}
