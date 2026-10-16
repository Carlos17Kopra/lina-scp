import { Download, Eye, FolderPlus, Pencil, RefreshCw, SquareTerminal, Trash2, Upload, X } from "lucide-react";
import { useState } from "react";
import { Button, cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { type Side, useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { hostProfile } from "../../model/hostFormat";
import { DeleteDialog } from "./DeleteDialog";
import { isDirLike } from "./entries";
import { FilePane, type PaneCommand } from "./FilePane";
import { NameDialog } from "./NameDialog";
import filePaneStyles from "./FilePane.module.css";
import { targets } from "./paneKeys";
import styles from "./SftpScreen.module.css";
import { dropDirAt, requestFor } from "./transferTargets";
import { useExternalDrop } from "./useExternalDrop";

type DialogState =
  | { type: "new"; side: Side }
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
    if (command === "newFolder") {
      setDialog({ type: "new", side: s });
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
  const toolbar = (command: PaneCommand) => onCommand(side)(command, targets(pane));

  return (
    <div className={styles.screen}>
      <div role="tablist" aria-label={t.sftp.tabs} className={styles.tabs}>
        {tabs.map((x) => (
          <div key={x.id} className={cx(styles.tab, x.id === tab.id && styles.activeTab)}>
            <button type="button" role="tab" aria-selected={x.id === tab.id} onClick={() => store().setActiveTab(x.id)}>
              {x.title}
            </button>
            <IconButton size="sm" icon={X} label={t.sftp.closeTab(x.title)} onClick={() => store().closeTab(x.id)} />
          </div>
        ))}
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
        <Button icon={Pencil} disabled={sideDisabled(side)} onClick={() => toolbar("rename")}>{t.sftp.rename}</Button>
        <Button icon={Trash2} disabled={sideDisabled(side)} onClick={() => toolbar("delete")}>{t.sftp.delete}</Button>
        <Button icon={RefreshCw} disabled={sideDisabled(side)} onClick={() => void store().reload(tab.id, side)}>{t.sftp.reload}</Button>
        <Button icon={Eye} aria-pressed={pane.showHidden} onClick={() => store().toggleHidden(tab.id, side)}>{t.sftp.hidden}</Button>
        <Button icon={SquareTerminal} onClick={() => void start(tab.hostId, "ssh")}>{t.sftp.terminal}</Button>
      </div>
      <div className={styles.panes}>
        <FilePane tabId={tab.id} side="local" onCommand={onCommand("local")} onTransfer={onTransfer("local")} />
        <div className={cx(styles.remote, externalOver && filePaneStyles.dropTarget)}>
          {lost ? (
            <div role="status" className={styles.lost}>
              <span>{t.sftp.lost}</span>
              <Button variant="primary" onClick={() => store().reconnect(tab.id)}>{t.sftp.reconnect}</Button>
            </div>
          ) : null}
          <FilePane tabId={tab.id} side="remote" disabled={lost} onCommand={onCommand("remote")} onTransfer={onTransfer("remote")} />
        </div>
      </div>
      <footer className={styles.status}>
        {t.sftp.status(`${user}@${host?.address ?? ""}`, tab.version, tab.info.cipher)}
      </footer>
      {dialog?.type === "new" ? (
        <NameDialog mode="new" onSubmit={(name) => store().mkdir(tab.id, dialog.side, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "rename" ? (
        <NameDialog mode="rename" initial={dialog.name} onSubmit={(name) => store().rename(tab.id, dialog.side, dialog.name, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "delete" ? (
        <DeleteDialog mode={dialog.mode} names={dialog.names} folder={dialog.folder}
          onConfirm={() => void store().remove(tab.id, dialog.side, dialog.names, dialog.mode)} onClose={() => setDialog(null)} />
      ) : null}
    </div>
  );
}
