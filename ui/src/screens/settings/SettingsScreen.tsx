import { Download, Upload } from "lucide-react";
import { useId, useState } from "react";
import { Button, Checkbox, Segmented, Select, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { Settings, SshMode, TerminalCursor, TerminalRenderer, Theme } from "../../ipc/api";
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
// Same list as core `TERMINAL_SCROLLBACKS`.
const SCROLLBACK_OPTIONS = [1000, 5000, 10000, 50000, 100000].map((lines) => ({
  value: String(lines),
  label: t.settings.scrollbackOption(lines),
}));
const CURSOR_OPTIONS: ReadonlyArray<{ value: TerminalCursor; label: string }> = [
  { value: "block", label: t.settings.cursors.block },
  { value: "bar", label: t.settings.cursors.bar },
  { value: "underline", label: t.settings.cursors.underline },
];

/**
 * A text field that saves on blur; `parse` returns `undefined` for input that must not be saved (it is reset).
 * It follows `value` (an import changes it under the open screen) and goes back to it when a save fails.
 */
function BlurField<T>({
  label,
  value,
  format,
  parse,
  onSave,
  ...rest
}: {
  label: string;
  value: T;
  format: (value: T) => string;
  parse: (text: string) => T | undefined;
  onSave: (value: T) => Promise<boolean>;
  placeholder?: string;
  hint?: string;
  mono?: boolean;
}) {
  const [text, setText] = useState(format(value));
  const [shown, setShown] = useState(value);
  if (value !== shown) {
    setShown(value);
    setText(format(value));
  }
  return (
    <TextField
      {...rest}
      label={label}
      spellCheck={false}
      value={text}
      onChange={setText}
      onBlur={() => {
        const next = parse(text);
        if (next === undefined) setText(format(value));
        else if (next !== value) void onSave(next).then((saved) => saved || setText(format(value)));
      }}
    />
  );
}

const optionalText = (text: string) => text.trim() || null;
const showOptional = (value: string | null) => value ?? "";
/** Whole numbers in `0..=max`; anything else is not saved. */
const wholeNumber = (min: number, max: number) => (text: string) => {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return value >= min && value <= max ? value : undefined;
};

/** Rail screen "Einstellungen"; every change is saved immediately (mirror changes after confirmation, text on blur). */
export function SettingsScreen() {
  const settings = useSettingsStore((state) => state.settings);
  const save = useSettingsStore((state) => state.save);
  const openModal = useUiStore((state) => state.openModal);
  const toast = useToast();
  const [pendingMode, setPendingMode] = useState<SshMode | null>(null);
  const ids = {
    appearance: useId(),
    ssh: useId(),
    terminal: useId(),
    local: useId(),
    newHosts: useId(),
    files: useId(),
    data: useId(),
  };

  const persist = (patch: Partial<Settings>) =>
    save(patch).then(
      () => true,
      (error: unknown) => {
        toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
        return false;
      },
    );

  const confirmMode = () => {
    if (pendingMode) void persist({ sshMode: pendingMode });
    setPendingMode(null);
  };

  return (
    <div className={styles.screen}>
      <h1 className={styles.title}>{t.settings.title}</h1>
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
            <Select
              label={t.settings.scrollback}
              value={String(settings.terminalScrollback)}
              options={SCROLLBACK_OPTIONS}
              onChange={(lines) => persist({ terminalScrollback: Number(lines) })}
            />
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>{t.settings.cursor}</span>
            <Segmented
              label={t.settings.cursor}
              value={settings.terminalCursor}
              options={CURSOR_OPTIONS}
              onChange={(terminalCursor) => persist({ terminalCursor })}
            />
          </div>
          <Checkbox
            label={t.settings.cursorBlink}
            checked={settings.terminalCursorBlink}
            onChange={(terminalCursorBlink) => persist({ terminalCursorBlink })}
          />
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
        <section className={styles.section} aria-labelledby={ids.local}>
          <h3 id={ids.local} className={styles.heading}>
            {t.settings.local}
          </h3>
          <BlurField
            label={t.settings.shell}
            hint={t.settings.shellHint}
            placeholder={t.settings.shellPlaceholder}
            mono
            value={settings.localShell}
            format={showOptional}
            parse={optionalText}
            onSave={(localShell) => persist({ localShell })}
          />
        </section>
        <section className={styles.section} aria-labelledby={ids.newHosts}>
          <h3 id={ids.newHosts} className={styles.heading}>
            {t.settings.newHosts}
          </h3>
          <p className={styles.hint}>{t.settings.newHostsHint}</p>
          <div className={styles.grid}>
            <BlurField
              label={t.settings.newHostUser}
              placeholder={t.settings.newHostUserPlaceholder}
              mono
              value={settings.newHostUser}
              format={showOptional}
              parse={optionalText}
              onSave={(newHostUser) => persist({ newHostUser })}
            />
            <BlurField
              label={t.settings.newHostPort}
              mono
              value={settings.newHostPort}
              format={String}
              parse={wholeNumber(1, 65535)}
              onSave={(newHostPort) => persist({ newHostPort })}
            />
            <BlurField
              label={t.settings.newHostKeepalive}
              hint={t.settings.newHostZeroHint}
              mono
              value={settings.newHostKeepaliveS}
              format={String}
              parse={wholeNumber(0, 3600)}
              onSave={(newHostKeepaliveS) => persist({ newHostKeepaliveS })}
            />
            <BlurField
              label={t.settings.newHostTimeout}
              hint={t.settings.newHostZeroHint}
              mono
              value={settings.newHostTimeoutS}
              format={String}
              parse={wholeNumber(0, 600)}
              onSave={(newHostTimeoutS) => persist({ newHostTimeoutS })}
            />
          </div>
        </section>
        <section className={styles.section} aria-labelledby={ids.files}>
          <h3 id={ids.files} className={styles.heading}>
            {t.settings.files}
          </h3>
          <BlurField
            label={t.settings.editor}
            hint={t.settings.editorHint}
            placeholder={t.settings.editorPlaceholder}
            mono
            value={settings.editorCommand}
            format={showOptional}
            parse={optionalText}
            onSave={(editorCommand) => persist({ editorCommand })}
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
    </div>
  );
}
