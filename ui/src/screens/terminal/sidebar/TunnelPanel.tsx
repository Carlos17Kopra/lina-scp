import { t } from "../../../i18n/de";
import { type ForwardKind, type PortForward, type TunnelState, tunnelStart, tunnelStop } from "../../../ipc/api";
import { errorMessage } from "../../../ipc/errors";
import { fireAndForget } from "../../../stores/connectProgress";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useTunnelsStore } from "../../../stores/tunnels";
import { useUiStore } from "../../../stores/ui";
import styles from "./Sidebar.module.css";

const BADGE: Record<ForwardKind, string> = { local: "L", remote: "R", dynamic: "D" };
const OFF: TunnelState = { kind: "off" };

/** Shows the defaults core applies (`parse_forward`): without an address, L/D bind to loopback, R asks for localhost. */
function withHost(bind: string, fallback: string): string {
  const text = bind.trim();
  return /^\d+$/.test(text) ? `${fallback}:${text}` : text;
}

function ruleText(rule: PortForward): string {
  if (rule.kind === "dynamic") return `${withHost(rule.bind, "127.0.0.1")} · ${t.terminal.tunnels.socks}`;
  const bind = withHost(rule.bind, rule.kind === "remote" ? "localhost" : "127.0.0.1");
  return `${bind} → ${rule.target.trim()}`;
}

function stateText(state: TunnelState, rule: PortForward): string {
  const texts = t.terminal.tunnels;
  switch (state.kind) {
    case "off":
      return texts.off;
    case "starting":
      return texts.starting;
    case "active":
      // Only a remote forward on port 0 has a port worth naming: the server picked it.
      return rule.kind === "remote" && withHost(rule.bind, "localhost").endsWith(":0") ? texts.activePort(state.port) : texts.active;
    case "failed":
      return texts.failed(errorMessage(state.error));
  }
}

/** Sidebar tab "Tunnel": the host's port-forward rules with their state and a start/stop switch. */
export function TunnelPanel({ tab }: { tab: Tab }) {
  const texts = t.terminal.tunnels;
  const states = useTunnelsStore((state) => state.states[tab.id]);
  const host = useHostsStore((state) => state.hosts.find((h) => h.id === tab.hostId));
  const openModal = useUiStore((state) => state.openModal);
  const rules = tab.forwards ?? [];
  const ready = tab.status === "connected" && tab.sessionId !== undefined;
  // The backend drops the tunnels of an ended session without further events.
  const ended = tab.status === "closed" || tab.status === "error";

  if (rules.length === 0) {
    return (
      <div className={styles.group}>
        <p className={styles.hint}>{texts.empty}</p>
        {tab.kind === "ssh" && host?.source === "linascp" ? (
          <button type="button" className={styles.link} onClick={() => openModal({ type: "editor", target: { kind: "host", id: host.id } })}>
            {texts.editHost}
          </button>
        ) : null}
      </div>
    );
  }

  return (
    <ul className={styles.tunnels}>
      {rules.map((rule) => {
        const state = (!ended && states?.[rule.id]) || OFF;
        const on = state.kind === "active" || state.kind === "starting";
        const toggle = () => {
          const sessionId = tab.sessionId;
          if (sessionId === undefined) return;
          // A failed start comes back as a "failed" event; nothing to add here.
          fireAndForget(() => (on ? tunnelStop(sessionId, rule.id) : tunnelStart(sessionId, rule.id)));
        };
        return (
          <li key={rule.id} className={styles.tunnel}>
            <span className={styles.badge}>{BADGE[rule.kind]}</span>
            <span className={styles.tunnelBody}>
              <span className={styles.mono}>{ruleText(rule)}</span>
              {state.kind === "failed" ? (
                <span role="alert" className={styles.tunnelError}>
                  {stateText(state, rule)}
                </span>
              ) : (
                <span className={styles.hint}>{stateText(state, rule)}</span>
              )}
            </span>
            <button type="button" className={styles.link} disabled={!ready} onClick={toggle}>
              {on ? texts.stop : texts.start}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
