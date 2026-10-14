import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { Button, Checkbox, Dialog, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import { exportFile } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useUiStore } from "../../stores/ui";
import styles from "./Settings.module.css";
import { ensureExtension, exportFileName, LINASCP_FILTER, validateExport } from "./transfer";

export function ExportDialog() {
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [includeKeys, setIncludeKeys] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [touched, setTouched] = useState({ passphrase: false, repeat: false });
  const [busy, setBusy] = useState(false);
  /** Key paths the finished export could not include; the dialog then stays open to show them. */
  const [skippedKeys, setSkippedKeys] = useState<string[] | null>(null);
  const errors = validateExport({ includeSecrets, passphrase, repeat });
  const invalid = Object.keys(errors).length > 0;
  const texts = t.settings.export;

  const run = async () => {
    if (invalid) return;
    let chosen: string | null;
    try {
      chosen = await saveDialog({ title: texts.saveTitle, defaultPath: exportFileName(), filters: [LINASCP_FILTER] });
    } catch (error) {
      toast.show({ tone: "red", message: t.editor.pickKeyFailed, details: errorDetails(error) });
      return;
    }
    if (!chosen) return;
    const path = ensureExtension(chosen);
    setBusy(true);
    try {
      const report = await exportFile(path, includeSecrets, includeSecrets && includeKeys, includeSecrets ? passphrase : null);
      toast.show({ tone: "green", message: texts.done(path) });
      if (report.skippedKeys.length > 0) setSkippedKeys(report.skippedKeys);
      else closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: texts.failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      size="md"
      title={texts.title}
      onClose={closeModal}
      footer={
        skippedKeys ? (
          <Button variant="primary" onClick={closeModal}>
            {t.settings.done}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={closeModal}>
              {t.common.cancel}
            </Button>
            <Button variant="primary" disabled={invalid || busy} onClick={() => void run()}>
              {texts.submit}
            </Button>
          </>
        )
      }
    >
      <div className={styles.sections}>
        {skippedKeys ? (
          <div role="status" aria-label={texts.skippedTitle} className={styles.confirm}>
            <span>{texts.skipped(skippedKeys.length)}</span>
            <ul className={styles.skippedKeys}>
              {skippedKeys.map((path) => (
                <li key={path} className="mono">
                  {path}
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        <p className={styles.hint}>{texts.intro}</p>
        <Checkbox label={texts.includeSecrets} hint={texts.includeSecretsHint} checked={includeSecrets} onChange={setIncludeSecrets} />
        {includeSecrets ? (
          <>
            <div className={styles.grid}>
              <TextField
                type="password"
                label={texts.passphrase}
                autoComplete="new-password"
                value={passphrase}
                error={touched.passphrase ? errors.passphrase : null}
                onChange={(value) => {
                  setPassphrase(value);
                  setTouched((current) => ({ ...current, passphrase: true }));
                }}
              />
              <TextField
                type="password"
                label={texts.repeat}
                autoComplete="new-password"
                value={repeat}
                error={touched.repeat ? errors.repeat : null}
                onChange={(value) => {
                  setRepeat(value);
                  setTouched((current) => ({ ...current, repeat: true }));
                }}
              />
            </div>
            <Checkbox label={texts.includeKeys} hint={texts.includeKeysHint} checked={includeKeys} onChange={setIncludeKeys} />
          </>
        ) : null}
      </div>
    </Dialog>
  );
}
