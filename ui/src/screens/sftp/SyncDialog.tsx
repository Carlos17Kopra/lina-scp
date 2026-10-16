import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Checkbox, Dialog, Segmented, TextField } from "../../components";
import { t } from "../../i18n/de";
import { type Difference, type DirContents, type NodeInfo, syncCancel, syncCompare, syncDelete, syncEnqueue, type SyncDeleteReport } from "../../ipc/api";
import { errorDetails, isCoreError } from "../../ipc/errors";
import { pushNotice } from "../../stores/notices";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { formatMtime, formatSize } from "./entries";
import { loadExclude, patterns, saveExclude } from "./excludePatterns";
import styles from "./SyncDialog.module.css";
import { planSync, requestsFor, type SyncAction, type SyncDirection } from "./syncPlan";

const reasonOf = (error: unknown) => (isCoreError(error) && "message" in error ? error.message : t.sftp.failed);

function nodeText(node: NodeInfo | null, contents: DirContents | null): string {
  if (!node) return "";
  if (node.dir) return contents ? t.sftp.sync.dirWith(contents.entries.length) : t.sftp.sync.dir;
  return `${formatSize(node.size)} · ${formatMtime(node.mtime)}`;
}

function reportDeletes(report: SyncDeleteReport) {
  const texts = t.sftp.sync;
  if (report.failed > 0 && report.first) {
    pushNotice({ tone: "red", message: texts.deleteFailed(report.failed, report.first.path, reasonOf(report.first.error)) });
  } else if (report.skipped > 0) {
    pushNotice({ tone: "amber", message: texts.deleteSkipped(report.skipped) });
  }
}

type View = { kind: "comparing" } | { kind: "result"; diffs: Difference[] } | { kind: "error"; error: unknown };

export function SyncDialog(props: {
  tabId: string;
  sessionId: string;
  hostId: string;
  localDir: string;
  remoteDir: string;
  onClose: () => void;
}) {
  // Frozen at open: the relative paths of a compare only mean something against the folders it compared.
  const [{ tabId, sessionId, hostId, localDir, remoteDir }] = useState(() => props);
  const { onClose } = props;
  const texts = t.sftp.sync;
  const [view, setView] = useState<View>({ kind: "comparing" });
  const [direction, setDirection] = useState<SyncDirection>("upload");
  const [deleteExtra, setDeleteExtra] = useState(false);
  const [exclude, setExclude] = useState(() => loadExclude(props.hostId));
  /** The patterns of the shown result; running with others would ignore what the user just excluded. */
  const [compared, setCompared] = useState(exclude);
  const stale = exclude !== compared;
  const [unchecked, setUnchecked] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const run = useRef(0);

  const startCompare = (text: string) => {
    const id = ++run.current;
    setCompared(text);
    setView({ kind: "comparing" });
    syncCompare(sessionId, localDir, remoteDir, patterns(text)).then(
      (diffs) => id === run.current && setView({ kind: "result", diffs }),
      (error: unknown) => id === run.current && setView({ kind: "error", error }),
    );
  };
  // Compare once on open; "Neu vergleichen" repeats it.
  useEffect(() => startCompare(exclude), []);

  const rows = useMemo(
    () => (view.kind === "result" ? planSync(view.diffs, direction, deleteExtra) : []),
    [view, direction, deleteExtra],
  );
  const chosen = rows.filter((r) => r.action && !unchecked.has(r.diff.rel));
  const counts = { upload: 0, download: 0, deleteLocal: 0, deleteRemote: 0 } satisfies Record<SyncAction, number>;
  for (const r of chosen) counts[r.action!] += 1;
  // Everything inside a folder goes with it.
  const remoteDeletes = chosen
    .filter((r) => r.action === "deleteRemote")
    .reduce((n, r) => n + 1 + (r.diff.contents?.entries.length ?? 0), 0);

  const cancel = () => {
    if (view.kind === "comparing") void syncCancel(sessionId).catch(() => {});
    onClose();
  };
  const recompare = () => {
    saveExclude(hostId, exclude);
    startCompare(exclude);
  };
  const toggle = (rel: string) =>
    setUnchecked((s) => {
      const next = new Set(s);
      if (!next.delete(rel)) next.add(rel);
      return next;
    });
  const submit = async () => {
    setBusy(true);
    const req = requestsFor(chosen);
    const ex = patterns(compared);
    const sftp = useSftpStore.getState();
    try {
      if (req.upload.length || req.download.length) useTransfersStore.getState().setView(tabId, "queue");
      if (req.upload.length) await syncEnqueue(sessionId, "upload", localDir, remoteDir, req.upload, ex);
      if (req.download.length) await syncEnqueue(sessionId, "download", localDir, remoteDir, req.download, ex);
      if (req.deleteRemote.length) {
        reportDeletes(await syncDelete(sessionId, "remote", remoteDir, req.deleteRemote, ex));
        void sftp.reload(tabId, "remote");
      }
      if (req.deleteLocal.length) {
        reportDeletes(await syncDelete(sessionId, "local", localDir, req.deleteLocal, ex));
        void sftp.reload(tabId, "local");
      }
    } catch (error) {
      pushNotice({ tone: "red", message: t.sftp.failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
      onClose();
    }
  };

  return (
    <Dialog
      open
      size="lg"
      title={texts.title}
      subtitle={`${localDir} ⇄ ${remoteDir}`}
      onClose={cancel}
      footer={
        <>
          <Button variant="ghost" onClick={cancel}>{t.common.cancel}</Button>
          <Button variant="primary" disabled={busy || stale || view.kind !== "result" || chosen.length === 0} onClick={() => void submit()}>
            {texts.run}
          </Button>
        </>
      }
    >
      <div className={styles.options}>
        <Segmented
          label={texts.direction}
          value={direction}
          options={(["upload", "download", "both"] as const).map((value) => ({ value, label: texts.directions[value] }))}
          onChange={setDirection}
        />
        <div className={styles.exclude}>
          <TextField label={texts.exclude} mono spellCheck={false} placeholder={texts.excludePlaceholder} value={exclude} onChange={setExclude} />
        </div>
        <Button onClick={recompare}>{texts.recompare}</Button>
      </div>
      <Checkbox label={texts.deleteExtra} checked={deleteExtra} disabled={direction === "both"} onChange={setDeleteExtra} />
      {view.kind === "comparing" ? <p role="status" className={styles.status}>{texts.comparing}</p> : null}
      {view.kind === "error" ? (
        <p role="alert" className={styles.error}>{`${texts.failed}: ${reasonOf(view.error)}`}</p>
      ) : null}
      {view.kind === "result" && rows.length === 0 ? <p role="status" className={styles.status}>{texts.equal}</p> : null}
      {rows.length > 0 ? (
        <div className={styles.list}>
          <table className={styles.table}>
            <thead>
              <tr>
                <td />
                <th scope="col">{texts.columns.action}</th>
                <th scope="col">{texts.columns.path}</th>
                <th scope="col">{texts.columns.local}</th>
                <th scope="col">{texts.columns.remote}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(({ diff, action, reason }) => (
                <tr key={diff.rel} className={action ? undefined : styles.skipped}>
                  <td>
                    <input
                      type="checkbox"
                      aria-label={texts.include(diff.rel)}
                      disabled={!action}
                      checked={Boolean(action) && !unchecked.has(diff.rel)}
                      onChange={() => toggle(diff.rel)}
                    />
                  </td>
                  <td>{action ? texts.actions[action] : texts.skipped[reason!]}</td>
                  <td className={`${styles.path} mono`}>{diff.rel}</td>
                  <td>{nodeText(diff.local, diff.local?.dir ? diff.contents : null)}</td>
                  <td>{nodeText(diff.remote, diff.remote?.dir ? diff.contents : null)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
      {stale && view.kind === "result" ? <p className={styles.summary}>{texts.stale}</p> : null}
      {remoteDeletes > 0 ? <p className={styles.warning}>{texts.remoteDeleteWarning(remoteDeletes)}</p> : null}
      {view.kind === "result" && rows.length > 0 ? <p className={styles.summary}>{texts.summary(counts)}</p> : null}
    </Dialog>
  );
}
