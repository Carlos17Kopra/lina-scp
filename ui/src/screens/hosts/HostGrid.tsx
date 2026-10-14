import { Plus } from "lucide-react";
import { useId, useMemo } from "react";
import { Button, Segmented } from "../../components";
import { t } from "../../i18n/de";
import { type ProtocolFilter, useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { filterHosts } from "./filterHosts";
import { HostCard } from "./HostCard";
import styles from "./HostGrid.module.css";

const FILTER_OPTIONS: ReadonlyArray<{ value: ProtocolFilter; label: string }> = [
  { value: "all", label: t.hosts.filterAll },
  { value: "ssh", label: t.hosts.filterSsh },
  { value: "sftp", label: t.hosts.filterSftp },
];

export function HostGrid() {
  const hosts = useHostsStore((state) => state.hosts);
  const groups = useHostsStore((state) => state.groups);
  const filter = useHostsStore((state) => state.filter);
  const query = useHostsStore((state) => state.query);
  const groupId = useHostsStore((state) => state.groupId);
  const selectedId = useHostsStore((state) => state.selectedId);
  const setFilter = useHostsStore((state) => state.setFilter);
  const select = useHostsStore((state) => state.select);
  const openModal = useUiStore((state) => state.openModal);
  const titleId = useId();

  const visible = useMemo(() => filterHosts(hosts, { filter, query, groupId }), [hosts, filter, query, groupId]);
  const title = (groupId !== null && groups.find((group) => group.id === groupId)?.name) || t.hosts.allHosts;

  return (
    <section className={styles.grid} aria-labelledby={titleId}>
      <div className={styles.head}>
        <div>
          <h2 id={titleId} className={styles.title}>
            {title}
          </h2>
          <p className={styles.count}>{t.hosts.count(visible.length)}</p>
        </div>
        <Segmented label={t.hosts.filter} value={filter} options={FILTER_OPTIONS} onChange={setFilter} size="sm" />
      </div>
      {hosts.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t.hosts.emptyTitle}</p>
          <p>{t.hosts.emptyText}</p>
          <Button variant="primary" icon={Plus} onClick={() => openModal({ type: "editor", target: { kind: "host" } })}>
            {t.hosts.newHost}
          </Button>
        </div>
      ) : visible.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t.hosts.noMatchTitle}</p>
          <p>{t.hosts.noMatchText}</p>
        </div>
      ) : (
        <ul className={styles.cards}>
          {visible.map((host) => (
            <li key={host.id}>
              <HostCard host={host} selected={host.id === selectedId} onSelect={() => select(host.id)} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
