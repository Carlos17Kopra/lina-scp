import { useEffect } from "react";
import { useToast } from "../components";
import { t } from "../i18n/de";
import { errorDetails } from "../ipc/errors";
import { HostsScreen } from "../screens/hosts/HostsScreen";
import { SftpPlaceholder } from "../screens/sftp/SftpPlaceholder";
import { SnippetsScreen } from "../screens/snippets/SnippetsScreen";
import { TerminalScreen } from "../screens/terminal/TerminalScreen";
import { useHostsStore } from "../stores/hosts";
import { useSettingsStore } from "../stores/settings";
import { useSnippetsStore } from "../stores/snippets";
import { useUiStore } from "../stores/ui";
import styles from "./AppShell.module.css";
import { ModalLayer } from "./ModalLayer";
import { Rail } from "./Rail";
import { useGlobalShortcuts } from "./useGlobalShortcuts";
import { useNoticeToasts } from "./useNoticeToasts";

/**
 * Rail + the views + modal layer. All views stay mounted (inactive ones are `hidden`)
 * so terminal sessions survive view switches.
 */
export function AppShell() {
  const view = useUiStore((state) => state.view);
  const setTheme = useUiStore((state) => state.setTheme);
  const openModal = useUiStore((state) => state.openModal);
  const theme = useSettingsStore((state) => state.settings.theme);
  const firstRunDone = useSettingsStore((state) => state.settings.firstRunDone);
  const settingsLoaded = useSettingsStore((state) => state.loaded);
  const loadSettings = useSettingsStore((state) => state.load);
  const loadHosts = useHostsStore((state) => state.load);
  const loadSnippets = useSnippetsStore((state) => state.load);
  const toast = useToast();

  useGlobalShortcuts();
  useNoticeToasts();

  useEffect(() => {
    const report = (error: unknown) =>
      toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) });
    loadSettings().catch(report);
    loadHosts().catch(report);
    loadSnippets().catch(report);
  }, [loadSettings, loadHosts, loadSnippets, toast]);

  useEffect(() => {
    setTheme(theme);
  }, [theme, setTheme]);

  useEffect(() => {
    if (settingsLoaded && !firstRunDone) openModal({ type: "firstRun" });
  }, [settingsLoaded, firstRunDone, openModal]);

  return (
    <div className={styles.shell}>
      <Rail />
      <main className={styles.main}>
        <section className={styles.view} hidden={view !== "hosts"}>
          <HostsScreen />
        </section>
        <section className={styles.view} hidden={view !== "terminal"}>
          <TerminalScreen />
        </section>
        <section className={styles.view} hidden={view !== "sftp"}>
          <SftpPlaceholder />
        </section>
        <section className={styles.view} hidden={view !== "snippets"}>
          <SnippetsScreen />
        </section>
      </main>
      <ModalLayer />
    </div>
  );
}
