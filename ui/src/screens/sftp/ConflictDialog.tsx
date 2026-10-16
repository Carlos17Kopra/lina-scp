import { useRef, useState } from "react";
import { Button, Checkbox, Dialog } from "../../components";
import { t } from "../../i18n/de";
import type { Decision } from "../../ipc/api";
import { useTransfersStore } from "../../stores/transfers";
import { formatMtime, formatSize } from "./entries";
import styles from "./SftpScreen.module.css";

/** One open conflict of the tab ("Datei existiert bereits"). Escape cancels the whole batch. */
export function ConflictDialog({ tabId }: { tabId: string }) {
  const conflict = useTransfersStore((s) => s.byTab[tabId]?.conflicts[0]);
  const job = useTransfersStore((s) => (conflict ? s.byTab[tabId]?.jobs[conflict.jobId] : undefined));
  const [all, setAll] = useState(false);
  const skipRef = useRef<HTMLButtonElement>(null);
  if (!conflict || !job) return null;
  const texts = t.sftp.conflict;
  const [newSide, oldSide] = job.direction === "upload" ? [t.sftp.local, t.sftp.remote] : [t.sftp.remote, t.sftp.local];
  const answer = (decision: Decision) => {
    setAll(false);
    void useTransfersStore.getState().resolve(tabId, decision, decision === "cancelBatch" ? false : all);
  };
  const name = job.rel.split("/").pop() ?? job.rel;
  return (
    <Dialog
      open
      size="sm"
      title={texts.title}
      onClose={() => answer("cancelBatch")}
      initialFocusRef={skipRef}
      footer={
        <>
          <Button variant="ghost" onClick={() => answer("cancelBatch")}>{t.common.cancel}</Button>
          <span className={styles.footerGap} />
          <Button ref={skipRef} onClick={() => answer("skip")}>{texts.skip}</Button>
          <Button onClick={() => answer("newer")}>{texts.newer}</Button>
          <Button variant="primary" onClick={() => answer("overwrite")}>{texts.overwrite}</Button>
        </>
      }
    >
      <p>{texts.text(name)}</p>
      <div className={styles.compare}>
        <div>
          <span>{texts.newSide(newSide)}</span>
          <span className="mono">{formatSize(conflict.source.size)}</span>
          <span className="mono">{formatMtime(conflict.source.mtime)}</span>
        </div>
        <div>
          <span>{texts.existingSide(oldSide)}</span>
          <span className="mono">{formatSize(conflict.target.size)}</span>
          <span className="mono">{formatMtime(conflict.target.mtime)}</span>
        </div>
      </div>
      <Checkbox label={texts.applyToAll} checked={all} onChange={setAll} />
    </Dialog>
  );
}
