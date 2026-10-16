import { Download, Eye, FilePen, FilePlus, FolderPlus, Pencil, RefreshCw, SquareTerminal, Trash2, Upload, X } from "lucide-react";
import { useState } from "react";
import { Button, cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { sftpReconnectNow, sftpStopReconnect } from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { conflictOf, dirtyCount, useEditsStore } from "../../stores/edits";
import { fireAndForget } from "../../stores/connectProgress";
import { useHostsStore } from "../../stores/hosts";
import { type Side, useSftpStore } from "../../stores/sftp";
import { openCount, useTransfersStore } from "../../stores/transfers";
import { ReconnectBar } from "../terminal/ReconnectBar";
import { CloseTabDialog } from "./CloseTabDialog";
import { ConflictDialog } from "./ConflictDialog";
import { EditConflictDialog } from "./EditConflictDialog";
import { hostProfile } from "../../model/hostFormat";
import { DeleteDialog } from "./DeleteDialog";
import { isDirLike, isFileLike, joinPath } from "./entries";
import { FilePane, type PaneCommand } from "./FilePane";
import { NameDialog } from "./NameDialog";
import { OpenWithDialog } from "./OpenWithDialog";
import filePaneStyles from "./FilePane.module.css";
import { targets } from "./paneKeys";
import styles from "./SftpScreen.module.css";
import { dropDirAt, requestFor } from "./transferTargets";
import { TransferPanel } from "./TransferPanel";
import { useExternalDrop } from "./useExternalDrop";

type DialogState =
  | { type: "new" | "newFile"; side: Side }
  | { type: "openWith"; side: Side; name: string }
  | { type: "rename"; side: Side; name: string }
  | { type: "delete"; side: Side; names: string[]; mode: "trash" | "permanent"; folder: boolean };

/** `onCancel` is offered only when there are open tabs to go back to. */
function HostPicker({ onPicked, onCancel }: { onPicked: () => void; onCancel?: () => void }) {
  const hosts = useHostsStore((s) => s.hosts).filter((h) => h.protocolSftp);
  const start = useConnectStore((s) => s.start);
  return (
    <div className={styles.picker}>
      <h2>{t.sftp.pickHost}</h2>
      {hosts.length === 0 ? <p>{t.sftp.noSftpHosts}</p> : null}
      <ul>
        {hosts.map((h) => (
          <li key={h.id}>
            <Button
              onClick={() => {
                onPicked();
                void start(h.id, "sftp");
              }}
            >
              {h.name}
            </Button>
          </li>
        ))}
      </ul>
      {onCancel ? (
        <Button variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
      ) : null}
    </div>
  );
}

export function SftpScreen() {
  const tabs = useSftpStore((s) => s.tabs);
  const activeTabId = useSftpStore((s) => s.activeTabId);
  const hosts = useHostsStore((s) => s.hosts);
  const profiles = useHostsStore((s) => s.profiles);
  const start = useConnectStore((s) => s.start);
  const [picking, setPicking] = useState(false);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [closing, setClosing] = useState<{ tabId: string; count: number } | null>(null);
  const byTab = useTransfersStore((s) => s.byTab);
  const edits = useEditsStore((s) => s.byTab);
  const tab = tabs.find((x) => x.id === activeTabId) ?? null;
  const store = useSftpStore.getState;
  const externalOver = useExternalDrop(tab?.id ?? "");

  if (!tab || picking) {
    const back = () => setPicking(false);
    return <HostPicker onPicked={back} onCancel={tab ? back : undefined} />;
  }
  const host = hosts.find((h) => h.id === tab.hostId);
  const user = host?.user?.trim() || (host ? hostProfile(host, profiles)?.user : "") || "";
  const lost = tab.status === "lost";
  const side = tab.active;
  const pane = tab.panes[side];
  const sideDisabled = (s: Side) => s === "remote" && tab.status !== "connected";

  const onTransfer = (from: Side) => (names: string[], to: Side, row: string | null) => {
    const request = requestFor(tab, from, names, row === null ? null : dropDirAt(tab, to, row));
    if (request) void useTransfersStore.getState().enqueue(tab.id, request.direction, request.sources, request.targetDir);
  };

  const onCommand = (s: Side) => (command: PaneCommand, names: string[]) => {
    if (sideDisabled(s)) return;
    if (command === "newFolder" || command === "newFile") {
      setDialog({ type: command === "newFolder" ? "new" : "newFile", side: s });
      return;
    }
    if (command === "rename") {
      setDialog({ type: "rename", side: s, name: names[0]! });
      return;
    }
    const entries = tab.panes[s].entries.filter((e) => names.includes(e.name));
    const folder = entries.some((e) => isDirLike(e) && e.kind.type !== "symlink");
    const mode = s === "local" && command === "delete" ? "trash" : "permanent";
    setDialog({ type: "delete", side: s, names, mode, folder });
  };
  const onEdit = (s: Side) => (name: string, how: "editor" | "default" | "choose") => {
    if (sideDisabled(s)) return;
    if (how === "choose") setDialog({ type: "openWith", side: s, name });
    else void useEditsStore.getState().open(tab.id, s, joinPath(tab.panes[s].path, name), { kind: how });
  };
  const editCursor = () => {
    const entry = pane.entries.find((e) => e.name === pane.cursor);
    if (entry && isFileLike(entry) && !entry.nameLossy) onEdit(side)(entry.name, "editor");
  };
  const requestClose = (tabId: string) => {
    const count = openCount(useTransfersStore.getState().byTab[tabId]) + dirtyCount(useEditsStore.getState().byTab[tabId]);
    if (count > 0) setClosing({ tabId, count });
    else store().closeTab(tabId);
  };
  const toolbar = (command: PaneCommand) => onCommand(side)(command, targets(pane));

  return (
    <div className={styles.screen}>
      <div role="tablist" aria-label={t.sftp.tabs} className={styles.tabs}>
        {tabs.map((x) => {
          // The conflict dialog shows only in the active tab; others get a dot.
          const pending = x.id !== tab.id && ((byTab[x.id]?.conflicts.length ?? 0) > 0 || conflictOf(edits[x.id]) !== undefined);
          return (
            <div key={x.id} className={cx(styles.tab, x.id === tab.id && styles.activeTab)}>
              <button
                type="button"
                role="tab"
                aria-selected={x.id === tab.id}
                aria-label={pending ? `${x.title} · ${t.sftp.pendingConflict}` : undefined}
                onClick={() => store().setActiveTab(x.id)}
              >
                {x.title}
                {pending ? <span className={styles.pendingDot} aria-hidden /> : null}
              </button>
              <IconButton size="sm" icon={X} label={t.sftp.closeTab(x.title)} onClick={() => requestClose(x.id)} />
            </div>
          );
        })}
        <IconButton size="sm" icon={FolderPlus} label={t.sftp.newTab} onClick={() => setPicking(true)} />
      </div>
      <div role="toolbar" aria-label={t.sftp.toolbar} className={styles.toolbar}>
        <Button
          icon={side === "local" ? Upload : Download}
          disabled={tab.status !== "connected"}
          onClick={() => onTransfer(side)(targets(pane), side === "local" ? "remote" : "local", null)}
        >
          {side === "local" ? t.sftp.upload : t.sftp.download}
        </Button>
        <Button icon={FolderPlus} disabled={sideDisabled(side)} onClick={() => toolbar("newFolder")}>{t.sftp.newFolder}</Button>
        <Button icon={FilePlus} disabled={sideDisabled(side)} onClick={() => toolbar("newFile")}>{t.sftp.newFile}</Button>
        <Button icon={Pencil} disabled={sideDisabled(side)} onClick={() => toolbar("rename")}>{t.sftp.rename}</Button>
        <Button icon={FilePen} disabled={sideDisabled(side)} onClick={editCursor}>{t.sftp.edit.button}</Button>
        <Button icon={Trash2} disabled={sideDisabled(side)} onClick={() => toolbar("delete")}>{t.sftp.delete}</Button>
        <Button icon={RefreshCw} disabled={sideDisabled(side)} onClick={() => void store().reload(tab.id, side)}>{t.sftp.reload}</Button>
        <Button icon={Eye} aria-pressed={pane.showHidden} onClick={() => store().toggleHidden(tab.id, side)}>{t.sftp.hidden}</Button>
        <Button icon={SquareTerminal} onClick={() => void start(tab.hostId, "ssh")}>{t.sftp.terminal}</Button>
      </div>
      <div className={styles.panes}>
        <FilePane tabId={tab.id} side="local" onCommand={onCommand("local")} onTransfer={onTransfer("local")} onEdit={onEdit("local")} />
        <div className={cx(styles.remote, externalOver && filePaneStyles.dropTarget)}>
          {tab.status === "reconnecting" && tab.reconnect ? (
            <ReconnectBar
              state={tab.reconnect}
              onNow={() => fireAndForget(() => sftpReconnectNow(tab.sessionId))}
              onStop={() => fireAndForget(() => sftpStopReconnect(tab.sessionId))}
            />
          ) : null}
          {lost ? (
            <div role="status" className={styles.lost}>
              <span>{t.sftp.lost}</span>
              <Button variant="primary" onClick={() => store().reconnect(tab.id)}>{t.sftp.reconnect}</Button>
            </div>
          ) : null}
          <FilePane tabId={tab.id} side="remote" disabled={tab.status !== "connected"} onCommand={onCommand("remote")} onTransfer={onTransfer("remote")} onEdit={onEdit("remote")} />
        </div>
      </div>
      <TransferPanel tabId={tab.id} />
      <footer className={styles.status}>
        {t.sftp.status(`${user}@${host?.address ?? ""}`, tab.version, tab.info.cipher)}
      </footer>
      {dialog?.type === "new" ? (
        <NameDialog mode="new" onSubmit={(name) => store().mkdir(tab.id, dialog.side, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "newFile" ? (
        <NameDialog mode="newFile" onSubmit={(name) => store().createFile(tab.id, dialog.side, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "openWith" ? (
        <OpenWithDialog name={dialog.name} onClose={() => setDialog(null)}
          onOpen={(how) => void useEditsStore.getState().open(tab.id, dialog.side, joinPath(tab.panes[dialog.side].path, dialog.name), how)} />
      ) : null}
      {dialog?.type === "rename" ? (
        <NameDialog mode="rename" initial={dialog.name} onSubmit={(name) => store().rename(tab.id, dialog.side, dialog.name, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "delete" ? (
        <DeleteDialog mode={dialog.mode} names={dialog.names} folder={dialog.folder}
          onConfirm={() => void store().remove(tab.id, dialog.side, dialog.names, dialog.mode)} onClose={() => setDialog(null)} />
      ) : null}
      {closing ? (
        <CloseTabDialog count={closing.count} onConfirm={() => store().closeTab(closing.tabId)} onClose={() => setClosing(null)} />
      ) : null}
      <ConflictDialog tabId={tab.id} />
      {/* One dialog at a time: transfer conflicts first. */}
      {(byTab[tab.id]?.conflicts.length ?? 0) === 0 ? <EditConflictDialog tabId={tab.id} /> : null}
    </div>
  );
}
