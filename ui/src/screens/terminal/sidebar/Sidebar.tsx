import { type ReactNode, useId, useState } from "react";
import { type TabItem, tabId, tabPanelId, Tabs } from "../../../components";
import { t } from "../../../i18n/de";
import type { Tab } from "../../../stores/sessions";
import { DesignPanel } from "./DesignPanel";
import { SessionPanel } from "./SessionPanel";
import styles from "./Sidebar.module.css";
import { SnippetsPanel } from "./SnippetsPanel";
import { TunnelPanel } from "./TunnelPanel";

type SidebarTab = "snippets" | "session" | "design" | "tunnel";

const TABS: readonly TabItem[] = [
  { id: "snippets", label: t.terminal.sidebar.snippets },
  { id: "session", label: t.terminal.sidebar.session },
  { id: "design", label: t.terminal.sidebar.design },
  { id: "tunnel", label: t.terminal.sidebar.tunnel },
];

const PANELS = {
  snippets: SnippetsPanel,
  session: SessionPanel,
  design: DesignPanel,
  tunnel: TunnelPanel,
} satisfies Record<SidebarTab, (props: { tab: Tab }) => ReactNode>;

/** Right-hand terminal sidebar for the active tab. */
export function Sidebar({ tab }: { tab: Tab }) {
  const [current, setCurrent] = useState<SidebarTab>("session");
  const idBase = useId();
  const Panel = PANELS[current];
  return (
    <aside className={styles.sidebar} aria-label={t.terminal.sidebar.label}>
      <Tabs
        label={t.terminal.sidebar.label}
        tabs={TABS}
        value={current}
        onChange={(id) => {
          if (id in PANELS) setCurrent(id as SidebarTab);
        }}
        idBase={idBase}
      />
      <div role="tabpanel" id={tabPanelId(idBase, current)} aria-labelledby={tabId(idBase, current)}>
        {tab.status === "picking" ? <p className={styles.hint}>{t.terminal.panes.noTerminal}</p> : <Panel tab={tab} />}
      </div>
    </aside>
  );
}
