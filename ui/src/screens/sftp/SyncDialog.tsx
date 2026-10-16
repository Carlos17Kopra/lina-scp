import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Checkbox, Dialog, Segmented, TextField } from "../../components";
import { t } from "../../i18n/de";
import { type Difference, type NodeInfo, syncCancel, syncCompare, syncDelete, syncEnqueue, type SyncDeleteReport } from "../../ipc/api";
import { errorDetails, isCoreError } from "../../ipc/errors";
import { pushNotice } from "../../stores/notices";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { formatMtime, formatSize } from "./entries";
import styles from "./SyncDialog.module.css";
import { planSync, requestsFor, type SyncAction, type SyncDirection } from "./syncPlan";

const excludeKey = (hostId: string) => `linascp.sync.exclude.${hostId}`;

function loadExclude(hostId: string): string {
  try {
    return localStorage.getItem(excludeKey(hostId)) ?? "";
  } catch {
    return "";
  }
}

function saveExclude(hostId: string, text: string) {
  try {
    localStorage.setItem(excludeKey(hostId), text);
  } catch {
    // ponytail: a remembered filter is a convenience; without storage it is simply not remembered.
  }
}

const patterns = (text: string) => text.split(",").map((p) => p.trim()).filter(Boolean);
const reasonOf = (error: unknown) => (isCoreError(error) && "message" in error ? error.message : t.sftp.failed);

function nodeText(node: NodeInfo | null): string {
  if (!node) return "";
  return node.dir ? t.sftp.sync.dir : `${formatSize(node.size)} · ${formatMtime(node.mtime)}`;
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

export function SyncDialog({ tabId, sessionId, hostId, localDir, remoteDir, onClose }: {
  tabId: string;
  sessionId: string;
  hostId: string;
  localDir: string;
  remoteDir: string;
  onClose: () => void;
}) {
  const texts = t.sftp.sync;
  const [view, setView] = useState<View>({ kind: "comparing" });
  const [direction, setDirection] = useState<SyncDirection>("upload");
  const [deleteExtra, setDeleteExtra] = useState(false);
  const [exclude, setExclude] = useState(() => loadExclude(hostId));
  const [unchecked, setUnchecked] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const run = useRef(0);

  const startCompare = (text: string) => {
    const id = ++run.current;
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
    const sftp = useSftpStore.getState();
    try {
      if (req.upload.length || req.download.length) useTransfersStore.getState().setView(tabId, "queue");
      if (req.upload.length) await syncEnqueue(sessionId, "upload", localDir, remoteDir, req.upload);
      if (req.download.length) await syncEnqueue(sessionId, "download", localDir, remoteDir, req.download);
      if (req.deleteRemote.length) {
        reportDeletes(await syncDelete(sessionId, "remote", remoteDir, req.deleteRemote));
        void sftp.reload(tabId, "remote");
      }
      if (req.deleteLocal.length) {
        reportDeletes(await syncDelete(sessionId, "local", localDir, req.deleteLocal));
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
          <Button variant="primary" disabled={busy || chosen.length === 0} onClick={() => void submit()}>
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
                  <td>{nodeText(diff.local)}</td>
                  <td>{nodeText(diff.remote)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
      {counts.deleteRemote > 0 ? <p className={styles.warning}>{texts.remoteDeleteWarning(counts.deleteRemote)}</p> : null}
      {view.kind === "result" && rows.length > 0 ? <p className={styles.summary}>{texts.summary(counts)}</p> : null}
    </Dialog>
  );
}
