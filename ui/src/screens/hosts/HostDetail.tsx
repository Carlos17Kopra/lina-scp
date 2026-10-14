import { FolderOpen, Pencil, SquareTerminal, Trash2 } from "lucide-react";
import { useId, useState } from "react";
import { Button, Chip, Dialog } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { formatEndpoint, hostColorVar, hostProfile, initials, methodLabel } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import styles from "./HostDetail.module.css";
import { useHostActions } from "./useHostActions";

export function HostDetail() {
  const host = useHostsStore((state) => state.hosts.find((h) => h.id === state.selectedId) ?? null);
  if (!host) {
    return (
      <aside className={styles.panel} aria-label={t.hosts.details}>
        <p className={styles.empty}>{t.hosts.noSelection}</p>
      </aside>
    );
  }
  return <HostDetailPanel key={host.id} host={host} />;
}

function HostDetailPanel({ host }: { host: Host }) {
  const hosts = useHostsStore((state) => state.hosts);
  const groups = useHostsStore((state) => state.groups);
  const profiles = useHostsStore((state) => state.profiles);
  const actions = useHostActions();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const forwardsId = useId();

  const group = groups.find((g) => g.id === host.groupId)?.name ?? t.hosts.noGroup;
  const subtitle = host.source === "sshConfig" ? `${group} · ${t.hosts.fromSshConfig}` : group;
  const auth = host.auth;
  const profileText =
    auth.kind === "profile"
      ? (hostProfile(host, profiles)?.name ?? t.hosts.none)
      : t.hosts.customAuth(methodLabel(auth.auth.method));
  const jump = host.jumpHostId ? (hosts.find((h) => h.id === host.jumpHostId)?.name ?? t.hosts.none) : t.hosts.none;
  const facts: Array<[string, string, boolean]> = [
    [t.hosts.address, formatEndpoint(host.user, host.address, 22), true],
    [t.hosts.port, String(host.port), true],
    [t.hosts.profile, profileText, false],
    [t.hosts.jumpHost, jump, false],
    [t.hosts.startPath, host.sftp.remoteStart ?? t.hosts.none, true],
    [t.hosts.fingerprint, t.hosts.none, true],
  ];

  const remove = async () => {
    setConfirmDelete(false);
    await actions.remove(host);
  };

  return (
    <aside className={styles.panel} aria-label={t.hosts.details}>
      <div className={styles.head}>
        <span className={styles.avatar} style={{ color: hostColorVar(host.color) }} aria-hidden="true">
          {initials(host.name)}
        </span>
        <div className={styles.titles}>
          <h2 className={styles.name}>{host.name}</h2>
          <p className={styles.subtitle}>{subtitle}</p>
        </div>
      </div>
      <div className={styles.actions}>
        <Button
          variant="primary"
          size="lg"
          icon={SquareTerminal}
          disabled={!host.protocolSsh}
          onClick={() => actions.connect(host)}
        >
          {t.hosts.terminal}
        </Button>
        <Button size="lg" icon={FolderOpen} aria-disabled="true" title={t.hosts.filesLater} className={styles.later}>
          {t.hosts.files}
        </Button>
      </div>
      <dl className={styles.facts}>
        {facts.map(([label, value, mono]) => (
          <div key={label} className={styles.fact}>
            <dt>{label}</dt>
            <dd className={mono ? "mono" : undefined}>{value}</dd>
          </div>
        ))}
      </dl>
      <section aria-labelledby={forwardsId}>
        <h3 id={forwardsId} className={styles.label}>
          {t.hosts.portForwarding}
        </h3>
        {host.forwards.length === 0 ? (
          <p className={styles.none}>{t.hosts.noRules}</p>
        ) : (
          <ul className={styles.rules}>
            {host.forwards.map((forward) => (
              <li key={forward.id} className={styles.rule}>
                <Chip tone="neutral" mono>
                  {t.hosts.forwardKind[forward.kind]}
                </Chip>
                <span className="mono">
                  {forward.kind === "dynamic" ? t.hosts.socks(forward.bind) : `${forward.bind} → ${forward.target}`}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
      <div className={styles.spacer} />
      <div className={styles.footer}>
        <Button icon={Pencil} onClick={() => actions.edit(host)}>
          {t.hosts.edit}
        </Button>
        {host.source === "linascp" ? (
          <Button variant="danger" icon={Trash2} onClick={() => setConfirmDelete(true)}>
            {t.hosts.delete}
          </Button>
        ) : null}
      </div>
      <Dialog
        open={confirmDelete}
        role="alertdialog"
        size="sm"
        title={t.hosts.deleteTitle}
        onClose={() => setConfirmDelete(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
              {t.common.cancel}
            </Button>
            <Button variant="danger" onClick={() => void remove()}>
              {t.common.delete}
            </Button>
          </>
        }
      >
        <p>{t.hosts.deleteText(host.name)}</p>
      </Dialog>
    </aside>
  );
}
