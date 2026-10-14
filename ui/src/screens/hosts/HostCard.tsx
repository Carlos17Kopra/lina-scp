import { FolderOpen, Pencil, SquareTerminal } from "lucide-react";
import { Card, Chip, IconButton } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { formatAddress, hostColorVar, initials } from "../../model/hostFormat";
import styles from "./HostCard.module.css";
import { useHostActions } from "./useHostActions";

export function HostCard({ host, selected, onSelect }: { host: Host; selected: boolean; onSelect: () => void }) {
  const actions = useHostActions();
  return (
    <Card selected={selected} className={styles.card}>
      <button
        type="button"
        className={styles.select}
        aria-pressed={selected}
        aria-label={t.hosts.select(host.name)}
        onClick={onSelect}
      >
        <span className={styles.avatar} style={{ color: hostColorVar(host.color) }} aria-hidden="true">
          {initials(host.name)}
        </span>
        <span className={styles.titles}>
          <span className={styles.name}>{host.name}</span>
          <span className={styles.address}>{formatAddress(host)}</span>
        </span>
      </button>
      <div className={styles.chips}>
        {host.protocolSsh ? <Chip tone="accent">{t.hosts.filterSsh}</Chip> : null}
        {host.protocolSftp ? <Chip tone="amber">{t.hosts.filterSftp}</Chip> : null}
        {host.source === "sshConfig" ? <Chip tone="neutral">{t.hosts.fromSshConfig}</Chip> : null}
        {host.tags.map((tag) => (
          <Chip key={tag} tone="neutral">
            {tag}
          </Chip>
        ))}
      </div>
      <div className={styles.footer}>
        {host.protocolSsh ? (
          <IconButton label={t.hosts.openTerminal} icon={SquareTerminal} size="sm" onClick={() => actions.connect(host)} />
        ) : null}
        {host.protocolSftp ? (
          <IconButton
            label={t.hosts.openFiles}
            icon={FolderOpen}
            size="sm"
            aria-disabled="true"
            title={t.hosts.filesLater}
            className={styles.later}
          />
        ) : null}
        <IconButton label={t.hosts.edit} icon={Pencil} size="sm" onClick={() => actions.edit(host)} />
      </div>
    </Card>
  );
}
