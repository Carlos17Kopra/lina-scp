import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { Button, Dialog, Segmented, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import { type Id, type ImportPreview, importApply, importPreview, type Resolution } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { useSettingsStore } from "../../stores/settings";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import styles from "./Settings.module.css";
import { defaultResolutions, isWrongPassphrase, LINASCP_FILTER, reportMessage } from "./transfer";

type Stage = { step: "pick" } | { step: "review"; path: string; preview: ImportPreview };

const RESOLUTION_OPTIONS: ReadonlyArray<{ value: Resolution; label: string }> = [
  { value: "skip", label: t.settings.import.resolutions.skip },
  { value: "replace", label: t.settings.import.resolutions.replace },
  { value: "duplicate", label: t.settings.import.resolutions.duplicate },
];

export function ImportDialog() {
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [stage, setStage] = useState<Stage>({ step: "pick" });
  const [resolutions, setResolutions] = useState<Record<Id, Resolution>>({});
  const [passphrase, setPassphrase] = useState("");
  const [passphraseError, setPassphraseError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const texts = t.settings.import;

  const pick = async () => {
    let path: string | string[] | null;
    try {
      path = await openDialog({ title: texts.pickTitle, multiple: false, directory: false, filters: [LINASCP_FILTER] });
    } catch (error) {
      toast.show({ tone: "red", message: t.common.pickFileFailed, details: errorDetails(error) });
      return;
    }
    if (typeof path !== "string") return;
    setBusy(true);
    try {
      const preview = await importPreview(path);
      setResolutions(defaultResolutions(preview));
      setStage({ step: "review", path, preview });
    } catch (error) {
      toast.show({ tone: "red", message: texts.readFailed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (stage.step !== "review") return;
    setBusy(true);
    setPassphraseError(null);
    try {
      const report = await importApply(stage.path, stage.preview.hasSecrets && passphrase ? passphrase : null, resolutions);
      toast.show({ tone: report.invalid > 0 ? "amber" : "green", message: reportMessage(report) });
      try {
        await Promise.all([
          useHostsStore.getState().load(),
          useSettingsStore.getState().load(),
          useSnippetsStore.getState().load(),
        ]);
      } catch (error) {
        toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) });
      }
      closeModal();
    } catch (error) {
      if (isWrongPassphrase(error)) setPassphraseError(texts.wrongPassphrase);
      else toast.show({ tone: "red", message: texts.failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  const footer =
    stage.step === "pick" ? (
      <>
        <Button variant="ghost" onClick={closeModal}>
          {t.common.cancel}
        </Button>
        <Button variant="primary" disabled={busy} onClick={() => void pick()}>
          {texts.pick}
        </Button>
      </>
    ) : (
      <>
        <Button variant="ghost" onClick={closeModal}>
          {t.common.cancel}
        </Button>
        <Button variant="primary" disabled={busy} onClick={() => void apply()}>
          {texts.submit}
        </Button>
      </>
    );

  return (
    <Dialog open size="md" title={texts.title} onClose={closeModal} footer={footer}>
      {stage.step === "pick" ? (
        <p className={styles.hint}>{texts.intro}</p>
      ) : (
        <div className={styles.sections}>
          <p className="mono">{stage.path}</p>
          <p className={styles.hint}>
            {texts.summary(stage.preview.items.length, stage.preview.items.filter((item) => item.conflict !== null).length)}
          </p>
          <ul className={styles.items}>
            {stage.preview.items.map((item) => (
              <li key={`${item.kind}-${item.id}`} className={styles.item}>
                <span className={styles.itemKind}>{texts.kinds[item.kind]}</span>
                <span className={styles.itemName}>{item.name}</span>
                {item.conflict ? (
                  <>
                    <span className={styles.conflict}>{texts.conflicts[item.conflict]}</span>
                    <Segmented
                      size="sm"
                      label={texts.resolve(item.name)}
                      value={resolutions[item.id] ?? "skip"}
                      options={RESOLUTION_OPTIONS}
                      onChange={(resolution) => setResolutions((current) => ({ ...current, [item.id]: resolution }))}
                    />
                  </>
                ) : (
                  <span className={styles.fresh}>{texts.noConflict}</span>
                )}
              </li>
            ))}
          </ul>
          {stage.preview.hasSecrets ? (
            <TextField
              type="password"
              label={texts.passphrase}
              hint={texts.passphraseHint}
              autoComplete="off"
              value={passphrase}
              error={passphraseError}
              onChange={(value) => {
                setPassphrase(value);
                setPassphraseError(null);
              }}
            />
          ) : null}
        </div>
      )}
    </Dialog>
  );
}
