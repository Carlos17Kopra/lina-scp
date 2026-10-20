import { useEffect, useMemo } from "react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { errorMessage } from "../../ipc/errors";
import { formatAddress } from "../../model/hostFormat";
import { useConnectStore } from "../../stores/connect";
import { fireAndForget } from "../../stores/connectProgress";
import { useHostsStore } from "../../stores/hosts";
import { type PfEntry, usePortForwardingStore } from "../../stores/portForwarding";
import { useTunnelsStore } from "../../stores/tunnels";
import { useUiStore } from "../../stores/ui";
import { TunnelRules } from "../terminal/sidebar/TunnelPanel";
import styles from "./PortForwarding.module.css";

/** Rail screen "Port-Forwarding": a host's rules over its own connection, without a terminal. */
export function PortForwardingScreen() {
  const texts = t.portForwarding;
  const hosts = useHostsStore((state) => state.hosts);
  const entries = usePortForwardingStore((state) => state.entries);
  const disconnect = usePortForwardingStore((state) => state.disconnect);
  const cards = useMemo(
    () => hosts.filter((h) => h.source === "linascp" && h.forwards.length > 0).sort((a, b) => a.name.localeCompare(b.name)),
    [hosts],
  );

  // A host that was deleted or lost its rules leaves the list; its connection must not run on unseen.
  useEffect(() => {
    for (const hostId of Object.keys(entries)) {
      if (!cards.some((h) => h.id === hostId)) fireAndForget(() => disconnect(hostId));
    }
  }, [cards, entries, disconnect]);

  return (
    <div className={styles.screen}>
      <header className={styles.header}>
        <h1 className={styles.title}>{texts.title}</h1>
      </header>
      {cards.length === 0 ? <p className={styles.empty}>{texts.empty}</p> : null}
      {cards.length > 0 ? (
        <ul className={styles.cards}>
          {cards.map((host) => (
            <HostTunnels key={host.id} host={host} entry={entries[host.id]} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function HostTunnels({ host, entry }: { host: Host; entry: PfEntry | undefined }) {
  const texts = t.portForwarding;
  const connecting = useConnectStore(
    (state) => state.active?.protocol === "tunnel" && state.active.hostId === host.id && !state.active.failed,
  );
  const start = useConnectStore((state) => state.start);
  const disconnect = usePortForwardingStore((state) => state.disconnect);
  const openModal = useUiStore((state) => state.openModal);
  const states = useTunnelsStore((state) => (entry ? state.states[entry.sessionId] : undefined));
  const status = entry?.status;
  const running = status?.kind === "connected" || status?.kind === "reconnecting";

  const stateLine = () => {
    if (connecting) return <span className={styles.meta}>{texts.connecting}</span>;
    switch (status?.kind) {
      case undefined:
        return <span className={styles.meta}>{texts.disconnected}</span>;
      case "connected":
        return <span className={styles.meta}>{texts.connected}</span>;
      case "reconnecting":
        return <span className={styles.meta}>{texts.reconnecting(status.attempt, status.maxAttempts)}</span>;
      case "lost":
        return (
          <span role="alert" className={styles.error}>
            {texts.lost(errorMessage(status.error))}
          </span>
        );
    }
  };

  return (
    <li className={styles.card}>
      <div className={styles.head}>
        <span className={styles.titles}>
          <h2 className={styles.name}>{host.name}</h2>
          <span className={styles.meta}>{formatAddress(host)}</span>
          {stateLine()}
        </span>
        {running ? (
          <Button size="sm" onClick={() => fireAndForget(() => disconnect(host.id))}>
            {texts.disconnect}
          </Button>
        ) : (
          <Button size="sm" variant="primary" disabled={connecting} onClick={() => void start(host.id, "tunnel")}>
            {texts.connect}
          </Button>
        )}
      </div>
      <TunnelRules
        rules={entry && status?.kind !== "lost" ? entry.forwards : host.forwards}
        states={status?.kind === "lost" ? undefined : states}
        ready={status?.kind === "connected"}
        sessionId={entry?.sessionId}
      />
      <button type="button" className={styles.link} onClick={() => openModal({ type: "editor", target: { kind: "host", id: host.id } })}>
        {texts.editHost}
      </button>
    </li>
  );
}
