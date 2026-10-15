import { Columns2, PanelRight, Plus, RadioTower, Rows2, X } from "lucide-react";
import { type KeyboardEvent, useRef } from "react";
import { Button, cx, IconButton, nextIndex } from "../../components";
import { t } from "../../i18n/de";
import { activeGroup, type Tab, type TabGroup, useSessionsStore } from "../../stores/sessions";
import { useSettingsStore } from "../../stores/settings";
import { MAX_PANES, paneIds } from "./layout";
import { toggleTerminalSidebar } from "./sidebar/toggle";
import styles from "./TabBar.module.css";

export const tabButtonId = (id: string) => `session-tab-${id}`;
export const tabPanelId = (id: string) => `session-panel-${id}`;

/** The pane a tab is named after: the focused one, unless it is still picking. */
function titlePane(group: TabGroup, tabs: readonly Tab[]): Tab | undefined {
  const panes = paneIds(group.layout).map((id) => tabs.find((tab) => tab.id === id));
  const focused = tabs.find((tab) => tab.id === group.focusedPaneId);
  return focused?.status !== "picking" ? focused : (panes.find((tab) => tab && tab.status !== "picking") ?? focused);
}

/** One tab per group plus split, broadcast, new local terminal and sidebar buttons. */
export function TabBar() {
  const tabs = useSessionsStore((state) => state.tabs);
  const groups = useSessionsStore((state) => state.groups);
  const activeId = useSessionsStore((state) => state.activeId);
  const active = useSessionsStore((state) => activeGroup(state));
  const setActive = useSessionsStore((state) => state.setActive);
  const closeGroup = useSessionsStore((state) => state.closeGroup);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const split = useSessionsStore((state) => state.split);
  const toggleBroadcast = useSessionsStore((state) => state.toggleBroadcast);
  const sidebarOpen = useSettingsStore((state) => state.settings.terminalSidebarOpen);
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const texts = t.terminal.panes;
  const activeCount = active ? paneIds(active.layout).length : 0;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, groups.length, "horizontal");
    const group = next === null ? undefined : groups[next];
    if (next === null || !group) return;
    event.preventDefault();
    setActive(group.id);
    refs.current[next]?.focus();
  };

  return (
    <div className={styles.bar}>
      <div role="tablist" aria-label={t.terminal.sessions} className={styles.tabs}>
        {groups.map((group, index) => {
          const pane = titlePane(group, tabs);
          const title = pane?.title ?? "";
          const count = paneIds(group.layout).length;
          const label = count > 1 ? texts.count(title, count) : title;
          const selected = group.id === activeId;
          const ended = pane?.status === "closed" || pane?.status === "error";
          const suffix =
            pane?.status === "reconnecting"
              ? t.terminal.reconnecting.suffixReconnecting
              : pane?.status === "lost"
                ? t.terminal.reconnecting.suffixLost
                : ended
                  ? t.terminal.endedSuffix
                  : null;
          return (
            <div key={group.id} className={cx(styles.tab, selected && styles.active)} data-status={pane?.status} data-kind={pane?.kind}>
              <button
                ref={(element) => {
                  refs.current[index] = element;
                }}
                type="button"
                role="tab"
                id={tabButtonId(group.id)}
                aria-selected={selected}
                aria-controls={tabPanelId(group.id)}
                tabIndex={selected ? 0 : -1}
                className={styles.tabButton}
                onClick={() => setActive(group.id)}
                onKeyDown={(event) => onKeyDown(event, index)}
              >
                <span className={styles.dot} aria-hidden="true" />
                <span className={styles.title}>{label}</span>
                {suffix ? <span className="visually-hidden">{` – ${suffix}`}</span> : null}
              </button>
              <IconButton
                size="sm"
                label={t.terminal.closeTab(title)}
                icon={X}
                className={styles.close}
                onClick={() => void closeGroup(group.id)}
              />
            </div>
          );
        })}
      </div>
      {active?.broadcast ? <span className={styles.broadcastState}>{texts.broadcastOn(activeCount)}</span> : null}
      <IconButton
        label={texts.splitRow}
        title={texts.splitRowTitle}
        icon={Columns2}
        disabled={!active || activeCount >= MAX_PANES}
        onClick={() => split("row")}
      />
      <IconButton
        label={texts.splitColumn}
        title={texts.splitColumnTitle}
        icon={Rows2}
        disabled={!active || activeCount >= MAX_PANES}
        onClick={() => split("column")}
      />
      <Button
        size="sm"
        variant="ghost"
        icon={RadioTower}
        title={texts.broadcastTitle}
        aria-pressed={!!active?.broadcast}
        disabled={activeCount < 2}
        className={cx(styles.broadcastButton, active?.broadcast && styles.broadcastOn)}
        onClick={toggleBroadcast}
      >
        {texts.broadcast}
      </Button>
      <IconButton label={t.terminal.newLocal} title={t.terminal.newLocalTitle} icon={Plus} onClick={() => void openLocal()} />
      <IconButton label={t.terminal.sidebar.toggle} icon={PanelRight} aria-pressed={sidebarOpen} onClick={toggleTerminalSidebar} />
    </div>
  );
}
