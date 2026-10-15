import { Copy } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { IconButton } from "../../../components";
import { t } from "../../../i18n/de";
import type { AuthMethod } from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { copyText } from "../clipboard";
import { useHost } from "../prefs";
import { formatSince } from "./format";
import styles from "./Sidebar.module.css";

const AUTH_LABELS: Record<AuthMethod, string> = {
  password: t.auth.password,
  key: t.auth.key,
  agent: t.auth.agent,
  keyboardInteractive: t.auth.keyboardInteractive,
};

const or = (value: string | null | undefined) => (value ? value : t.terminal.info.none);

function Row({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className={styles.row}>
      <dt className={styles.term}>{term}</dt>
      <dd className={styles.value}>{children}</dd>
    </div>
  );
}

/** Re-renders once a minute so "Verbunden seit" stays current. */
function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

/** Sidebar tab "Sitzung": connection facts of the active tab. */
export function SessionPanel({ tab }: { tab: Tab }) {
  const host = useHost(tab.hostId);
  const hosts = useHostsStore((state) => state.hosts);
  const profiles = useHostsStore((state) => state.profiles);
  const now = useMinuteClock();
  const ended = tab.status === "closed" || tab.status === "error";
  const size = tab.cols && tab.rows ? `xterm-256color · ${tab.cols}×${tab.rows}` : "xterm-256color";
  const since = ended ? t.terminal.info.disconnected : tab.connectedAt ? formatSince(tab.connectedAt, now) : t.terminal.info.none;

  if (tab.kind === "local") {
    return (
      <dl className={styles.list}>
        <Row term={t.terminal.info.type}>{t.terminal.info.localShell}</Row>
        <Row term={t.terminal.info.terminal}>{size}</Row>
        <Row term={t.terminal.info.openedSince}>{since}</Row>
      </dl>
    );
  }

  const profileId = host?.auth.kind === "profile" ? host.auth.profileId : undefined;
  const profile = profileId === undefined ? undefined : profiles.find((entry) => entry.profile.id === profileId)?.profile;
  // Same order as core's `effective_user` (the local $USER fallback is not known here).
  const user = host?.user?.trim() ? host.user : profile?.user;
  const address = host ? `${user ? `${user}@` : ""}${host.address}:${host.port}` : null;
  const jump = host?.jumpHostId ? hosts.find((candidate) => candidate.id === host.jumpHostId)?.name : null;
  const info = tab.info;
  const methods = info?.authMethods.map((method) => AUTH_LABELS[method]).join(" + ");
  const login = methods ? (profile ? `${methods} · ${t.terminal.info.profile(profile.name)}` : methods) : null;
  const keepalive = host ? (host.keepaliveS > 0 ? t.terminal.info.keepaliveEvery(host.keepaliveS) : t.terminal.info.keepaliveOff) : null;

  return (
    <dl className={styles.list}>
      <Row term={t.terminal.info.host}>
        <span className={styles.mono}>{or(address)}</span>
      </Row>
      <Row term={t.terminal.info.jump}>{or(jump)}</Row>
      <Row term={t.terminal.info.login}>{or(login)}</Row>
      <Row term={t.terminal.info.hostKey}>
        {info?.hostKeySha256 ? (
          <>
            <span className={styles.mono}>{`${info.hostKeyAlgorithm} ${info.hostKeySha256}`}</span>
            <IconButton size="sm" label={t.terminal.info.copyFingerprint} icon={Copy} onClick={() => void copyText(info.hostKeySha256)} />
          </>
        ) : (
          t.terminal.info.none
        )}
      </Row>
      <Row term={t.terminal.info.kex}>
        <span className={styles.mono}>{or(info?.kex)}</span>
      </Row>
      <Row term={t.terminal.info.cipher}>
        <span className={styles.mono}>{info?.cipher ? `${info.cipher} · ${or(info.mac)}` : t.terminal.info.none}</span>
      </Row>
      <Row term={t.terminal.info.terminal}>{size}</Row>
      <Row term={t.terminal.info.keepalive}>{or(keepalive)}</Row>
      <Row term={t.terminal.info.connectedSince}>{since}</Row>
    </dl>
  );
}
