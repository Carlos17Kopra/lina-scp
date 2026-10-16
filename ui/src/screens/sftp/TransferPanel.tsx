import { ChevronDown, ChevronUp, RotateCcw, X } from "lucide-react";
import { Button, cx, IconButton, ProgressBar } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Job } from "../../ipc/api";
import { openCount, speedOf, type TransferView, useTransfersStore, viewOf } from "../../stores/transfers";
import { formatSize } from "./entries";
import styles from "./TransferPanel.module.css";

const VIEWS: TransferView[] = ["queue", "done", "failed"];

function errorText(error: CoreError): string {
  const texts = t.sftp.transfers;
  if (error.type === "fs") return t.sftp.fsErrors[error.kind];
  if (error.type === "network") return texts.networkError;
  if (error.type === "inputRequired") return texts.inputRequired;
  return t.sftp.failed;
}

function stateText(job: Job): string {
  const texts = t.sftp.transfers;
  if (job.state === "failed" && job.error) return errorText(job.error);
  if (job.state === "skipped" && job.note) return texts.notes[job.note];
  if (job.state === "running") return `${job.size > 0 ? Math.floor((job.done * 100) / job.size) : 100} %`;
  return texts.states[job.state];
}

const parentDir = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";

export function TransferPanel({ tabId }: { tabId: string }) {
  const tab = useTransfersStore((s) => s.byTab[tabId]);
  const store = useTransfersStore.getState;
  if (!tab || tab.order.length === 0) return null;
  const texts = t.sftp.transfers;
  const counts = { queue: 0, done: 0, failed: 0 };
  for (const id of tab.order) counts[viewOf(tab.jobs[id]!.state)]++;
  const now = Date.now();
  const rows = tab.order.map((id) => tab.jobs[id]!).filter((j) => viewOf(j.state) === tab.view);

  return (
    <section aria-label={texts.title} className={styles.panel}>
      <header className={styles.head}>
        {tab.collapsed ? (
          <span className={styles.summary}>{texts.summary(openCount(tab), counts.done, counts.failed)}</span>
        ) : (
          <div role="tablist" aria-label={texts.title} className={styles.views}>
            {VIEWS.map((view) => (
              <button key={view} type="button" role="tab" aria-selected={tab.view === view}
                className={cx(styles.view, tab.view === view && styles.activeView)}
                onClick={() => store().setView(tabId, view)}>
                {`${texts.views[view]} ${counts[view]}`}
              </button>
            ))}
          </div>
        )}
        <span className={styles.spacer} />
        {!tab.collapsed && tab.view === "done" && counts.done > 0 ? (
          <Button size="sm" variant="ghost" onClick={() => void store().clear(tabId)}>{texts.clear}</Button>
        ) : null}
        <IconButton size="sm" icon={tab.collapsed ? ChevronUp : ChevronDown}
          label={tab.collapsed ? texts.expand : texts.collapse} onClick={() => store().toggleCollapsed(tabId)} />
      </header>
      {tab.collapsed ? null : (
        <div className={styles.list}>
          {rows.length === 0 ? <p className={styles.empty}>{texts.empty}</p> : null}
          {rows.map((job) => {
            const speed = job.state === "running" ? speedOf(tab.samples[job.id] ?? [], now) : null;
            const percent = job.size > 0 ? Math.floor((job.done * 100) / job.size) : job.state === "done" ? 100 : 0;
            return (
              <div key={job.id} className={styles.row}>
                <span aria-hidden className={job.direction === "upload" ? styles.up : styles.down}>
                  {job.direction === "upload" ? "↑" : "↓"}
                </span>
                <span className={styles.name} title={job.source}>{job.rel}</span>
                <span className={cx(styles.target, "mono")} title={job.target}>{parentDir(job.target)}</span>
                <ProgressBar value={percent / 100} tone={job.state === "failed" ? "red" : job.state === "done" ? "green" : "accent"} label={job.rel} />
                <span className={cx(styles.speed, "mono")}>{speed === null ? "—" : texts.speed(formatSize(Math.round(speed)))}</span>
                <span className={styles.state}>{stateText(job)}</span>
                {viewOf(job.state) === "queue" ? (
                  <IconButton size="sm" icon={X} label={texts.cancel(job.rel)} onClick={() => store().cancel(tabId, { type: "job", id: job.id })} />
                ) : job.state === "failed" ? (
                  <IconButton size="sm" icon={RotateCcw} label={texts.retry(job.rel)} onClick={() => store().retry(tabId, job.id)} />
                ) : <span />}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
