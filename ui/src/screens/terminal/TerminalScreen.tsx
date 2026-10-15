import { Plus } from "lucide-react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { focusedPane, useSessionsStore } from "../../stores/sessions";
import { useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { PaneLayout } from "./PaneLayout";
import { Sidebar } from "./sidebar/Sidebar";
import { TabBar, tabButtonId, tabPanelId } from "./TabBar";
import styles from "./TerminalScreen.module.css";

/** Terminal view: tab bar + one panel per tab (group) with its split panes. Stays mounted while hidden (AppShell). */
export function TerminalScreen() {
  const groups = useSessionsStore((state) => state.groups);
  const activeId = useSessionsStore((state) => state.activeId);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const shown = useUiStore((state) => state.view === "terminal");
  const sidebarOpen = useSettingsStore((state) => state.settings.terminalSidebarOpen);
  const pane = useSessionsStore((state) => focusedPane(state));

  return (
    <div className={styles.screen}>
      <h1 className="visually-hidden">{t.terminal.title}</h1>
      {groups.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t.terminal.emptyTitle}</p>
          <p>{t.terminal.emptyText}</p>
          <Button variant="primary" icon={Plus} onClick={() => void openLocal()}>
            {t.terminal.openLocal}
          </Button>
        </div>
      ) : (
        <>
          <TabBar />
          <div className={styles.body}>
            <div className={styles.panels}>
              {groups.map((group) => (
                <div
                  key={group.id}
                  role="tabpanel"
                  id={tabPanelId(group.id)}
                  aria-labelledby={tabButtonId(group.id)}
                  hidden={group.id !== activeId}
                  className={styles.panel}
                >
                  <PaneLayout group={group} visible={shown && group.id === activeId} />
                </div>
              ))}
            </div>
            {sidebarOpen && pane ? <Sidebar tab={pane} /> : null}
          </div>
        </>
      )}
    </div>
  );
}
