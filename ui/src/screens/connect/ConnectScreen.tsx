import { type KeyboardEvent, useEffect, useRef } from "react";
import { t } from "../../i18n/de";
import type { HostKeyDecision } from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import styles from "./Connect.module.css";
import { ConnectProgress } from "./ConnectProgress";
import { ConnectTabBar } from "./ConnectTabBar";
import { connectView } from "./connectText";
import { ErrorAuth } from "./ErrorAuth";
import { ErrorConnection } from "./ErrorConnection";
import { HostKeyChangedDialog } from "./HostKeyChangedDialog";
import { HostKeyNewDialog } from "./HostKeyNewDialog";

/**
 * Connection overlay right of the rail (loading, host-key decisions, error screens).
 * Rendered by the modal layer while `modal.type === "connect"`; Escape cancels.
 */
export function ConnectScreen() {
  const flow = useConnectStore((state) => state.active);
  const cancel = useConnectStore((state) => state.cancel);
  const answer = useConnectStore((state) => state.answer);
  const host = useHostsStore((state) => (flow ? state.hosts.find((h) => h.id === flow.hostId) : undefined));
  const overlay = useRef<HTMLElement>(null);
  const view = flow ? connectView(flow) : null;
  const hostMissing = !!flow && !host;

  // The host vanished (deleted / ssh-config reload): do not trap the user in an empty overlay.
  useEffect(() => {
    if (hostMissing) void cancel();
  }, [hostMissing, cancel]);

  // Keep keyboard focus inside the overlay so Escape cancels; dialogs with their own autofocus win.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-run when the shown screen changes
  useEffect(() => {
    const element = overlay.current;
    if (element && !element.contains(document.activeElement)) element.focus();
  }, [view, hostMissing]);

  if (!flow || !host || !view) return null;


  const openEditor = (target: { kind: "host" | "profile"; id: string }) => {
    void cancel();
    useUiStore.getState().openModal({ type: "editor", target });
  };
  const editHost = () => openEditor({ kind: "host", id: host.id });
  const decide = (promptId: number) => (decision: HostKeyDecision) => void answer(promptId, { kind: "hostKey", decision });
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    void cancel();
  };

  const pending = flow.prompt;
  let body = <ConnectProgress flow={flow} host={host} />;
  if (pending?.prompt.kind === "hostKeyUnknown") {
    const { prompt } = pending;
    body = <HostKeyNewDialog host={prompt.host} port={prompt.port} info={prompt.info} onDecision={decide(pending.promptId)} />;
  } else if (pending?.prompt.kind === "hostKeyChanged") {
    const { prompt } = pending;
    body = (
      <HostKeyChangedDialog
        key={pending.promptId}
        host={prompt.host}
        port={prompt.port}
        stored={prompt.stored}
        current={prompt.current}
        onDecision={decide(pending.promptId)}
        onEditHost={editHost}
      />
    );
  } else if (view === "errorAuth" && flow.failed) {
    body = <ErrorAuth flow={flow} host={host} error={flow.failed.error} onEdit={openEditor} />;
  } else if (view === "errorConnection" && flow.failed) {
    body = <ErrorConnection flow={flow} host={host} error={flow.failed.error} onEditHost={editHost} />;
  }

  return (
    <section ref={overlay} tabIndex={-1} className={styles.overlay} aria-label={t.connect.label(host.name)} onKeyDown={onKeyDown}>
      <ConnectTabBar name={host.name} view={view} busy={flow.retrying} />
      <div className={styles.content}>{body}</div>
    </section>
  );
}
