import {
  ArrowRightLeft,
  CodeXml,
  FolderSync,
  KeyRound,
  type LucideIcon,
  Moon,
  Server,
  Settings,
  SquareTerminal,
  Sun,
} from "lucide-react";
import { cx, useToast } from "../components";
import { t } from "../i18n/de";
import { errorDetails } from "../ipc/errors";
import { useSettingsStore } from "../stores/settings";
import { useUiStore, type View } from "../stores/ui";
import styles from "./Rail.module.css";

const VIEW_ITEMS: ReadonlyArray<{ view: View; label: string; icon: LucideIcon }> = [
  { view: "hosts", label: t.rail.hosts, icon: Server },
  { view: "terminal", label: t.rail.terminal, icon: SquareTerminal },
  { view: "sftp", label: t.rail.sftp, icon: FolderSync },
  { view: "snippets", label: t.rail.snippets, icon: CodeXml },
];

const LATER_ITEMS: ReadonlyArray<{ label: string; icon: LucideIcon }> = [
  { label: t.rail.keychain, icon: KeyRound },
  { label: t.rail.portForwarding, icon: ArrowRightLeft },
];

const ICON_PROPS = { size: 20, strokeWidth: 1.8, "aria-hidden": true } as const;

export function Rail() {
  const view = useUiStore((state) => state.view);
  const setView = useUiStore((state) => state.setView);
  const openModal = useUiStore((state) => state.openModal);
  const theme = useUiStore((state) => state.theme);
  const setTheme = useUiStore((state) => state.setTheme);
  const saveSettings = useSettingsStore((state) => state.save);
  const toast = useToast();

  const toggleTheme = () => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    saveSettings({ theme: next }).catch((error: unknown) =>
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) }),
    );
  };
  // The toggle names (and shows the icon of) the mode it switches to.
  const themeLabel = theme === "dark" ? t.rail.toLight : t.rail.toDark;
  const ThemeIcon = theme === "dark" ? Sun : Moon;

  return (
    <nav className={styles.rail} aria-label={t.rail.nav}>
      <div className={styles.logo} aria-hidden="true">
        &gt;_
      </div>
      {VIEW_ITEMS.map(({ view: target, label, icon: Icon }) => (
        <button
          key={target}
          type="button"
          className={cx(styles.item, view === target && styles.active)}
          aria-label={label}
          title={label}
          aria-current={view === target ? "page" : undefined}
          onClick={() => setView(target)}
        >
          <Icon {...ICON_PROPS} />
        </button>
      ))}
      {LATER_ITEMS.map(({ label, icon: Icon }) => (
        <button
          key={label}
          type="button"
          className={cx(styles.item, styles.disabled)}
          aria-label={label}
          aria-disabled="true"
          title={`${label} – ${t.common.comingSoon}`}
        >
          <Icon {...ICON_PROPS} />
        </button>
      ))}
      <div className={styles.spacer} />
      <button
        type="button"
        className={styles.item}
        aria-label={t.rail.settings}
        title={t.rail.settings}
        onClick={() => openModal({ type: "settings" })}
      >
        <Settings {...ICON_PROPS} />
      </button>
      <button type="button" className={styles.item} aria-label={themeLabel} title={themeLabel} onClick={toggleTheme}>
        <ThemeIcon {...ICON_PROPS} />
      </button>
    </nav>
  );
}
