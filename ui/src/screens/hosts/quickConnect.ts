import type { Host } from "../../ipc/api";
import { DEFAULT_PORT, newHost, QUICK_CONNECT_TAG } from "../../model/defaults";
import { formatEndpoint } from "../../model/hostFormat";

export interface QuickTarget {
  user?: string;
  host: string;
  port: number;
}

const USER_RE = /^[^\s@:/[\]]+$/;
const HOSTNAME_RE = /^[A-Za-z0-9._-]+$/;
/** IPv6 literal, optionally with a zone id (`fe80::1%eth0`). */
const IPV6_RE = /^[0-9A-Fa-f:.]+(%[\w.-]+)?$/;

function parsePort(text: string | undefined): number | null {
  if (text === undefined) return DEFAULT_PORT;
  if (!/^\d{1,5}$/.test(text)) return null;
  const port = Number(text);
  return port >= 1 && port <= 65535 ? port : null;
}

/** Parses `[user@]host[:port]`, `[user@][ipv6][:port]` or a bare IPv6 address (port 22). */
export function parseQuickConnect(input: string): QuickTarget | null {
  const text = input.trim();
  if (!text) return null;
  const at = text.lastIndexOf("@");
  const user = at === -1 ? undefined : text.slice(0, at);
  const rest = at === -1 ? text : text.slice(at + 1);
  if (user !== undefined && !USER_RE.test(user)) return null;

  let host: string;
  let portText: string | undefined;
  if (rest.startsWith("[")) {
    const close = rest.indexOf("]");
    if (close === -1) return null;
    host = rest.slice(1, close);
    const after = rest.slice(close + 1);
    if (after) {
      if (!after.startsWith(":")) return null;
      portText = after.slice(1);
    }
    if (!host.includes(":") || !IPV6_RE.test(host)) return null;
  } else if ((rest.match(/:/g) ?? []).length > 1) {
    host = rest;
    if (!IPV6_RE.test(host)) return null;
  } else {
    const colon = rest.indexOf(":");
    host = colon === -1 ? rest : rest.slice(0, colon);
    portText = colon === -1 ? undefined : rest.slice(colon + 1);
    if (!HOSTNAME_RE.test(host)) return null;
  }

  const port = parsePort(portText);
  if (port === null) return null;
  return user ? { user, host, port } : { host, port };
}

export function formatQuickTarget(target: QuickTarget): string {
  return formatEndpoint(target.user, target.host, target.port);
}

export function findQuickConnectHost(hosts: readonly Host[], target: QuickTarget): Host | undefined {
  return hosts.find(
    (host) =>
      host.tags.includes(QUICK_CONNECT_TAG) &&
      host.address === target.host &&
      host.port === target.port &&
      (host.user ?? undefined) === target.user,
  );
}

/** Host saved for a Quick Connect target: SSH only, password asked inline while connecting. */
export function quickConnectHost(target: QuickTarget): Host {
  return newHost({
    name: formatQuickTarget(target),
    address: target.host,
    port: target.port,
    user: target.user ?? null,
    tags: [QUICK_CONNECT_TAG],
    protocolSftp: false,
  });
}
