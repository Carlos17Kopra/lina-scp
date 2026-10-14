import { KeyRound, Plus } from "lucide-react";
import { useId } from "react";
import { cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { methodLabel } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import styles from "./HostsSidebar.module.css";

export function HostsSidebar() {
  const hosts = useHostsStore((state) => state.hosts);
  const groups = useHostsStore((state) => state.groups);
  const profiles = useHostsStore((state) => state.profiles);
  const groupId = useHostsStore((state) => state.groupId);
  const setGroupId = useHostsStore((state) => state.setGroupId);
  const openModal = useUiStore((state) => state.openModal);
  const groupsId = useId();
  const profilesId = useId();

  const items = [
    { id: null, name: t.hosts.allHosts, count: hosts.length },
    ...[...groups]
      .sort((a, b) => a.sortOrder - b.sortOrder || a.name.localeCompare(b.name))
      .map((group) => ({ id: group.id, name: group.name, count: hosts.filter((host) => host.groupId === group.id).length })),
  ];

  return (
    <aside className={styles.sidebar} aria-label={t.hosts.sidebar}>
      <section className={styles.section} aria-labelledby={groupsId}>
        <div className={styles.heading}>
          <h2 id={groupsId} className={styles.label}>
            {t.hosts.groups}
          </h2>
        </div>
        <ul className={styles.list}>
          {items.map((item) => {
            const active = item.id === groupId;
            return (
              <li key={item.id ?? "all"}>
                <button
                  type="button"
                  className={cx(styles.item, active && styles.active)}
                  aria-pressed={active}
                  onClick={() => setGroupId(item.id)}
                >
                  <span>{item.name}</span>
                  <span className={styles.count}>{item.count}</span>
                </button>
              </li>
            );
          })}
        </ul>
      </section>
      <section className={styles.section} aria-labelledby={profilesId}>
        <div className={styles.heading}>
          <h2 id={profilesId} className={styles.label}>
            {t.hosts.profiles}
          </h2>
          <IconButton
            label={t.hosts.addProfile}
            icon={Plus}
            size="sm"
            onClick={() => openModal({ type: "editor", target: { kind: "profile" } })}
          />
        </div>
        {profiles.length === 0 ? <p className={styles.empty}>{t.hosts.noProfiles}</p> : null}
        <ul className={styles.list}>
          {profiles.map(({ profile, usedBy }) => (
            <li key={profile.id}>
              <button
                type="button"
                className={cx(styles.item, styles.profile)}
                onClick={() => openModal({ type: "editor", target: { kind: "profile", id: profile.id } })}
              >
                <span className={styles.profileIcon} aria-hidden="true">
                  <KeyRound size={14} strokeWidth={2} />
                </span>
                <span className={styles.profileText}>
                  <span className={styles.profileName}>{profile.name}</span>
                  <span className={styles.profileMeta}>{t.hosts.profileMeta(methodLabel(profile.auth.method), usedBy)}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    </aside>
  );
}
