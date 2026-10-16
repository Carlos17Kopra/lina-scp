import { useState } from "react";
import { Button, Dialog, TextField } from "../../components";
import { t } from "../../i18n/de";
import { keepStart } from "../../ipc/api";
import { isCoreError } from "../../ipc/errors";
import { loadExclude, patterns, saveExclude } from "./excludePatterns";
import styles from "./SyncDialog.module.css";

const reasonOf = (error: unknown) => (isCoreError(error) && "message" in error ? error.message : t.sftp.failed);

export function KeepDialog(props: { sessionId: string; hostId: string; localDir: string; remoteDir: string; onClose: () => void }) {
  const { sessionId, hostId, localDir, remoteDir, onClose } = props;
  const texts = t.sftp.keep;
  const [exclude, setExclude] = useState(() => loadExclude(hostId));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const submit = async () => {
    setBusy(true);
    saveExclude(hostId, exclude);
    try {
      await keepStart(sessionId, localDir, remoteDir, patterns(exclude));
      onClose();
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      title={texts.title}
      subtitle={`${localDir} → ${remoteDir}`}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="primary" disabled={busy} onClick={() => void submit()}>{texts.start}</Button>
        </>
      }
    >
      <p className={styles.status}>{texts.hint}</p>
      <TextField label={t.sftp.sync.exclude} mono spellCheck={false} placeholder={t.sftp.sync.excludePlaceholder} value={exclude} onChange={setExclude} />
      {error ? <p role="alert" className={styles.error}>{`${texts.failed}: ${reasonOf(error)}`}</p> : null}
    </Dialog>
  );
}
