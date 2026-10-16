import { Download, Upload } from "lucide-react";
import { useId, useState } from "react";
import { Button, Checkbox, Dialog, Segmented, Select, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { Settings, SshMode, TerminalRenderer, Theme } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { AUTO_SCHEME, FONT_SIZE_MAX, FONT_SIZE_MIN, isKnownScheme, SCHEME_CHOICES } from "../terminal/theme";
import styles from "./Settings.module.css";

const THEME_OPTIONS: ReadonlyArray<{ value: Theme; label: string }> = [
  { value: "dark", label: t.settings.themes.dark },
  { value: "light", label: t.settings.themes.light },
];
const MODE_OPTIONS: ReadonlyArray<{ value: SshMode; label: string }> = [
  { value: "integrate", label: t.settings.modes.integrate },
  { value: "mirror", label: t.settings.modes.mirror },
];
const RENDERER_OPTIONS: ReadonlyArray<{ value: TerminalRenderer; label: string }> = [
  { value: "auto", label: t.settings.renderers.auto },
  { value: "webgl", label: t.settings.renderers.webgl },
  { value: "dom", label: t.settings.renderers.dom },
];
const FONT_OPTIONS = Array.from({ length: FONT_SIZE_MAX - FONT_SIZE_MIN + 1 }, (_, index) => {
  const size = FONT_SIZE_MIN + index;
  return { value: String(size), label: t.settings.fontSizeOption(size) };
});

const SCHEME_OPTIONS = SCHEME_CHOICES.map((choice) => ({ value: choice.id, label: choice.label }));

/** Minimal TP1 settings; every change is saved immediately (mirror changes after confirmation). */
export function SettingsDialog() {
  const settings = useSettingsStore((state) => state.settings);
  const save = useSettingsStore((state) => state.save);
  const closeModal = useUiStore((state) => state.closeModal);
  const openModal = useUiStore((state) => state.openModal);
  const toast = useToast();
  const [pendingMode, setPendingMode] = useState<SshMode | null>(null);
  const ids = { appearance: useId(), ssh: useId(), terminal: useId(), data: useId() };

  const persist = (patch: Partial<Settings>) => {
    save(patch).catch((error: unknown) =>
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) }),
    );
  };

  const confirmMode = () => {
    if (pendingMode) persist({ sshMode: pendingMode });
    setPendingMode(null);
  };

  return (
    <Dialog
      open
      size="md"
      title={t.settings.title}
      onClose={closeModal}
      footer={
        <Button variant="primary" onClick={closeModal}>
          {t.settings.done}
        </Button>
      }
    >
      <div className={styles.sections}>
        <section className={styles.section} aria-labelledby={ids.appearance}>
          <h3 id={ids.appearance} className={styles.heading}>
            {t.settings.appearance}
          </h3>
          <div className={styles.row}>
            <span className={styles.rowLabel}>{t.settings.theme}</span>
            <Segmented label={t.settings.theme} value={settings.theme} options={THEME_OPTIONS} onChange={(theme) => persist({ theme })} />
          </div>
        </section>
        <section className={styles.section} aria-labelledby={ids.ssh}>
          <h3 id={ids.ssh} className={styles.heading}>
            {t.settings.ssh}
          </h3>
          <div className={styles.row}>
            <span className={styles.rowLabel}>{t.settings.sshMode}</span>
            <Segmented
              label={t.settings.sshMode}
              value={pendingMode ?? settings.sshMode}
              options={MODE_OPTIONS}
              onChange={(mode) => setPendingMode(mode === settings.sshMode ? null : mode)}
            />
          </div>
          <p className={styles.hint}>{settings.sshMode === "mirror" ? t.settings.mirrorHint : t.settings.integrateHint}</p>
          {pendingMode ? (
            <div className={styles.confirm}>
              <p>{pendingMode === "mirror" ? t.settings.mirrorConfirm : t.settings.unmirrorConfirm}</p>
              <div className={styles.confirmActions}>
                <Button variant="ghost" size="sm" onClick={() => setPendingMode(null)}>
                  {t.common.cancel}
                </Button>
                <Button variant="primary" size="sm" onClick={confirmMode}>
                  {pendingMode === "mirror" ? t.settings.enableMirror : t.settings.disableMirror}
                </Button>
              </div>
            </div>
          ) : null}
        </section>
        <section className={styles.section} aria-labelledby={ids.terminal}>
          <h3 id={ids.terminal} className={styles.heading}>
            {t.settings.terminal}
          </h3>
          <div className={styles.grid}>
            <Select
              label={t.settings.renderer}
              value={settings.terminalRenderer}
              options={RENDERER_OPTIONS}
              onChange={(terminalRenderer) => persist({ terminalRenderer })}
            />
            <Select
              label={t.settings.fontSize}
              value={String(settings.terminalFontSize)}
              options={FONT_OPTIONS}
              onChange={(size) => persist({ terminalFontSize: Number(size) })}
            />
            <Select
              label={t.terminal.design.scheme}
              value={isKnownScheme(settings.terminalScheme) ? settings.terminalScheme : AUTO_SCHEME}
              options={SCHEME_OPTIONS}
              onChange={(terminalScheme) => persist({ terminalScheme })}
            />
          </div>
          <Checkbox
            label={t.settings.compat}
            hint={t.settings.compatHint}
            checked={settings.compatRendering}
            onChange={(compatRendering) => persist({ compatRendering })}
          />
          <Checkbox
            label={t.settings.autoReconnect}
            checked={settings.autoReconnect}
            onChange={(autoReconnect) => persist({ autoReconnect })}
          />
        </section>
        <section className={styles.section} aria-labelledby={ids.data}>
          <h3 id={ids.data} className={styles.heading}>
            {t.settings.data}
          </h3>
          <p className={styles.hint}>{t.settings.dataHint}</p>
          <div className={styles.buttons}>
            <Button icon={Upload} onClick={() => openModal({ type: "export" })}>
              {t.settings.exportButton}
            </Button>
            <Button icon={Download} onClick={() => openModal({ type: "import" })}>
              {t.settings.importButton}
            </Button>
          </div>
        </section>
      </div>
    </Dialog>
  );
}
