import { useId, useState } from "react";
import { type TabItem, tabId, tabPanelId, Tabs } from "../../../components";
import { t } from "../../../i18n/de";
import type { Tab } from "../../../stores/sessions";
import { DesignPanel } from "./DesignPanel";
import { SessionPanel } from "./SessionPanel";
import styles from "./Sidebar.module.css";

type SidebarTab = "session" | "design";

const TABS: readonly TabItem[] = [
  { id: "session", label: t.terminal.sidebar.session },
  { id: "design", label: t.terminal.sidebar.design },
  { id: "tunnel", label: t.terminal.sidebar.tunnel, disabled: true },
];

/** Right-hand terminal sidebar for the active tab (Snippets follow in TP2b, Tunnel in TP5). */
export function Sidebar({ tab }: { tab: Tab }) {
  const [current, setCurrent] = useState<SidebarTab>("session");
  const idBase = useId();
  return (
    <aside className={styles.sidebar} aria-label={t.terminal.sidebar.label}>
      <Tabs
        label={t.terminal.sidebar.label}
        tabs={TABS}
        value={current}
        onChange={(id) => setCurrent(id === "design" ? "design" : "session")}
        idBase={idBase}
      />
      <div role="tabpanel" id={tabPanelId(idBase, current)} aria-labelledby={tabId(idBase, current)}>
        {current === "session" ? <SessionPanel tab={tab} /> : <DesignPanel tab={tab} />}
      </div>
      <p className={styles.hint}>{t.terminal.sidebar.tunnelLater}</p>
    </aside>
  );
}
