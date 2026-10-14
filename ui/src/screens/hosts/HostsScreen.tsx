import { t } from "../../i18n/de";
import { HostDetail } from "./HostDetail";
import { HostGrid } from "./HostGrid";
import styles from "./HostsScreen.module.css";
import { HostsSidebar } from "./HostsSidebar";
import { HostsToolbar } from "./HostsToolbar";

/** Server overview: groups/profiles sidebar, toolbar (search, Quick Connect), host grid, detail panel. */
export function HostsScreen() {
  return (
    <div className={styles.screen}>
      <h1 className="visually-hidden">{t.hosts.title}</h1>
      <HostsSidebar />
      <div className={styles.main}>
        <HostsToolbar />
        <HostGrid />
      </div>
      <HostDetail />
    </div>
  );
}
