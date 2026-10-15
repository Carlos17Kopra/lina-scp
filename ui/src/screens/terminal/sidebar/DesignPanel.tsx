import { Minus, Plus } from "lucide-react";
import { IconButton, Segmented, useToast } from "../../../components";
import { t } from "../../../i18n/de";
import { errorDetails } from "../../../ipc/errors";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useSettingsStore } from "../../../stores/settings";
import { useUiStore } from "../../../stores/ui";
import { effectivePrefs, useHost } from "../prefs";
import { AUTO_SCHEME, FONT_SIZE_MAX, FONT_SIZE_MIN, isKnownScheme, resolveScheme, SCHEME_CHOICES } from "../theme";
import styles from "./Sidebar.module.css";

type Scope = "all" | "host";

/** Sidebar tab "Design": scheme + font size, globally or as an override of the tab's host. */
export function DesignPanel({ tab }: { tab: Tab }) {
  const host = useHost(tab.hostId);
  const saveHost = useHostsStore((state) => state.saveHost);
  const settings = useSettingsStore((state) => state.settings);
  const saveSettings = useSettingsStore((state) => state.save);
  const appTheme = useUiStore((state) => state.theme);
  const toast = useToast();
  const { scheme, fontSize } = effectivePrefs(host, settings);
  const selected = isKnownScheme(scheme) ? scheme : AUTO_SCHEME;
  const scope: Scope = host && (host.terminalScheme != null || host.terminalFontSize != null) ? "host" : "all";
  const blockedReason =
    tab.kind === "local"
      ? t.terminal.design.scopeLocal
      : !host
        ? t.terminal.design.scopeDeleted
        : host.source !== "linascp"
          ? t.terminal.design.scopeSshConfig
          : null;

  const fail = (error: unknown) => toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });

  // In host scope both fields are written: "Nur diesen Host" pins both effective values.
  const apply = (next: { scheme: string; fontSize: number }) => {
    if (scope === "host" && host) {
      saveHost({ ...host, terminalScheme: next.scheme, terminalFontSize: next.fontSize }).catch(fail);
    } else {
      saveSettings({ terminalScheme: next.scheme, terminalFontSize: next.fontSize }).catch(fail);
    }
  };

  const changeScope = (next: Scope) => {
    if (!host || next === scope) return;
    const keep = next === "host";
    saveHost({ ...host, terminalScheme: keep ? scheme : null, terminalFontSize: keep ? fontSize : null }).catch(fail);
  };

  return (
    <div className={styles.design}>
      <section className={styles.group} aria-label={t.terminal.design.scheme}>
        <h3 className={styles.heading}>{t.terminal.design.scheme}</h3>
        {/* ponytail: plain tabbable radios, no roving arrow keys; add when the list grows */}
        <div role="radiogroup" aria-label={t.terminal.design.scheme} className={styles.schemes}>
          {SCHEME_CHOICES.map((choice) => {
            const preview = resolveScheme(choice.id, appTheme);
            return (
              <button
                key={choice.id}
                type="button"
                role="radio"
                aria-checked={selected === choice.id}
                className={styles.scheme}
                onClick={() => apply({ scheme: choice.id, fontSize })}
              >
                <span className={styles.preview} style={{ background: preview.bg, color: preview.fg }} aria-hidden="true">
                  <span style={{ color: preview.green }}>user@host</span>:<span style={{ color: preview.blue }}>~</span>$ ls{" "}
                  <span style={{ color: preview.yellow }}>warn</span> <span style={{ color: preview.red }}>error</span>
                </span>
                <span className={styles.schemeName}>{choice.label}</span>
              </button>
            );
          })}
        </div>
      </section>
      <section className={styles.group} aria-label={t.terminal.design.fontSize}>
        <h3 className={styles.heading}>{t.terminal.design.fontSize}</h3>
        <div className={styles.fontRow}>
          <IconButton
            label={t.terminal.design.smaller}
            icon={Minus}
            disabled={fontSize <= FONT_SIZE_MIN}
            onClick={() => apply({ scheme, fontSize: fontSize - 1 })}
          />
          <span aria-live="polite">{t.settings.fontSizeOption(fontSize)}</span>
          <IconButton
            label={t.terminal.design.bigger}
            icon={Plus}
            disabled={fontSize >= FONT_SIZE_MAX}
            onClick={() => apply({ scheme, fontSize: fontSize + 1 })}
          />
        </div>
      </section>
      <section className={styles.group} aria-label={t.terminal.design.scope}>
        <h3 className={styles.heading}>{t.terminal.design.scope}</h3>
        <Segmented<Scope>
          label={t.terminal.design.scope}
          size="sm"
          value={scope}
          options={[
            { value: "all", label: t.terminal.design.scopeAll },
            { value: "host", label: t.terminal.design.scopeHost, disabled: blockedReason !== null },
          ]}
          onChange={changeScope}
        />
        {blockedReason ? <p className={styles.hint}>{blockedReason}</p> : null}
      </section>
    </div>
  );
}
