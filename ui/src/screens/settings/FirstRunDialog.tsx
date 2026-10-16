import { useRef, useState } from "react";
import { Button, Dialog, OptionCards, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { SshMode } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import styles from "./Settings.module.css";

const OPTIONS: ReadonlyArray<{ value: SshMode; label: string; hint: string }> = [
  { value: "integrate", label: t.settings.modes.integrate, hint: t.settings.firstRun.integrateHint },
  { value: "mirror", label: t.settings.modes.mirror, hint: t.settings.firstRun.mirrorHint },
];

/** First start: choose how LinaSCP treats ~/.ssh (Integrieren preselected); saves `firstRunDone`. */
export function FirstRunDialog() {
  const save = useSettingsStore((state) => state.save);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [mode, setMode] = useState<SshMode>("integrate");
  const [saving, setSaving] = useState(false);
  // State lags a fast second Escape; the ref does not.
  const inFlight = useRef(false);

  const finish = async (sshMode: SshMode) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true);
    try {
      await save({ sshMode, firstRunDone: true });
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      size="md"
      hideClose
      title={t.settings.firstRun.title}
      subtitle={t.settings.firstRun.subtitle}
      onClose={() => void finish("integrate")}
      footer={
        <>
          <span className={styles.footnote}>{t.settings.firstRun.later}</span>
          <span className={styles.spacer} />
          <Button variant="primary" disabled={saving} onClick={() => void finish(mode)}>
            {t.settings.firstRun.start}
          </Button>
        </>
      }
    >
      <div className={styles.sections}>
        <OptionCards label={t.settings.firstRun.label} value={mode} options={OPTIONS} onChange={setMode} />
        {mode === "mirror" ? <p className={styles.confirm}>{t.settings.mirrorConfirm}</p> : null}
      </div>
    </Dialog>
  );
}
