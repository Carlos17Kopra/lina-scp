import { Plus, X } from "lucide-react";
import { type KeyboardEvent, useRef } from "react";
import { cx, IconButton, nextIndex } from "../../components";
import { t } from "../../i18n/de";
import { useSessionsStore } from "../../stores/sessions";
import styles from "./TabBar.module.css";

export const tabButtonId = (id: string) => `session-tab-${id}`;
export const tabPanelId = (id: string) => `session-panel-${id}`;

/** Session tabs (one per connection or local terminal) plus "+" for a new local terminal. */
export function TabBar() {
  const tabs = useSessionsStore((state) => state.tabs);
  const activeId = useSessionsStore((state) => state.activeId);
  const setActive = useSessionsStore((state) => state.setActive);
  const closeSession = useSessionsStore((state) => state.closeSession);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, tabs.length, "horizontal");
    const tab = next === null ? undefined : tabs[next];
    if (next === null || !tab) return;
    event.preventDefault();
    setActive(tab.id);
    refs.current[next]?.focus();
  };

  return (
    <div className={styles.bar}>
      <div role="tablist" aria-label={t.terminal.sessions} className={styles.tabs}>
        {tabs.map((tab, index) => {
          const active = tab.id === activeId;
          const ended = tab.status === "closed" || tab.status === "error";
          return (
            <div key={tab.id} className={cx(styles.tab, active && styles.active)} data-status={tab.status} data-kind={tab.kind}>
              <button
                ref={(element) => {
                  refs.current[index] = element;
                }}
                type="button"
                role="tab"
                id={tabButtonId(tab.id)}
                aria-selected={active}
                aria-controls={tabPanelId(tab.id)}
                tabIndex={active ? 0 : -1}
                className={styles.tabButton}
                onClick={() => setActive(tab.id)}
                onKeyDown={(event) => onKeyDown(event, index)}
              >
                <span className={styles.dot} aria-hidden="true" />
                <span className={styles.title}>{tab.title}</span>
                {ended ? <span className="visually-hidden">{` – ${t.terminal.endedSuffix}`}</span> : null}
              </button>
              <IconButton
                size="sm"
                label={t.terminal.closeTab(tab.title)}
                icon={X}
                className={styles.close}
                onClick={() => void closeSession(tab.id)}
              />
            </div>
          );
        })}
      </div>
      <IconButton label={t.terminal.newLocal} title={t.terminal.newLocalTitle} icon={Plus} onClick={() => void openLocal()} />
    </div>
  );
}
