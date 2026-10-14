import { Plus } from "lucide-react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { useSessionsStore } from "../../stores/sessions";
import { useUiStore } from "../../stores/ui";
import { SessionEnded } from "./SessionEnded";
import { TabBar, tabButtonId, tabPanelId } from "./TabBar";
import styles from "./TerminalScreen.module.css";
import { TerminalView } from "./TerminalView";

/** Terminal view: tab bar + one panel per session. Stays mounted while hidden (AppShell). */
export function TerminalScreen() {
  const tabs = useSessionsStore((state) => state.tabs);
  const activeId = useSessionsStore((state) => state.activeId);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const shown = useUiStore((state) => state.view === "terminal");

  return (
    <div className={styles.screen}>
      <h1 className="visually-hidden">{t.terminal.title}</h1>
      {tabs.length === 0 ? (
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
          <div className={styles.panels}>
            {tabs.map((tab) => (
              <div
                key={tab.id}
                role="tabpanel"
                id={tabPanelId(tab.id)}
                aria-labelledby={tabButtonId(tab.id)}
                hidden={tab.id !== activeId}
                className={styles.panel}
              >
                {tab.sessionId ? <TerminalView tab={tab} visible={shown && tab.id === activeId} /> : null}
                {tab.status === "closed" || tab.status === "error" ? <SessionEnded tab={tab} /> : null}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
