import { t } from "../i18n/de";
import type { AuthConfig, AuthMethod, Host, LoginProfile, ProfileWithUsage } from "../ipc/api";
import { DEFAULT_PORT } from "./defaults";

/** Host accent colors (design tokens); stored in `Host.color` by name. */
export const HOST_COLORS = ["accent", "blue", "violet", "amber", "pink", "muted"] as const;
export type HostColor = (typeof HOST_COLORS)[number];

export function isHostColor(value: unknown): value is HostColor {
  return typeof value === "string" && (HOST_COLORS as readonly string[]).includes(value);
}

export function hostColorVar(color: string | null): string {
  return `var(--${isHostColor(color) ? color : "accent"})`;
}

/** "web-prod-01" → "WP", "bastion" → "BA". */
export function initials(name: string): string {
  const parts = name.split(/[-_.\s]+/).filter((part) => part.length > 0);
  const letters = parts.length > 1 ? parts.map((part) => part[0]).join("") : (parts[0] ?? "").slice(0, 2);
  return letters.slice(0, 2).toUpperCase() || "?";
}

/** `user@address[:port]`; the port is omitted when it is 22 unless `alwaysPort`. IPv6 is bracketed with a port. */
export function formatEndpoint(
  user: string | null | undefined,
  address: string,
  port: number,
  alwaysPort = false,
): string {
  const showPort = alwaysPort || port !== DEFAULT_PORT;
  const host = showPort && address.includes(":") ? `[${address}]` : address;
  return `${user ? `${user}@` : ""}${host}${showPort ? `:${port}` : ""}`;
}

export function formatAddress(host: Pick<Host, "user" | "address" | "port">): string {
  return formatEndpoint(host.user, host.address, host.port);
}

export function methodLabel(method: AuthMethod): string {
  return t.auth[method];
}

export function hostProfile(host: Host, profiles: readonly ProfileWithUsage[]): LoginProfile | null {
  const auth = host.auth;
  if (auth.kind !== "profile") return null;
  return profiles.find((entry) => entry.profile.id === auth.profileId)?.profile ?? null;
}

/** Effective auth config (individual or from the login profile); `null` if the profile is missing. */
export function hostAuthConfig(host: Host, profiles: readonly ProfileWithUsage[]): AuthConfig | null {
  return host.auth.kind === "custom" ? host.auth.auth : (hostProfile(host, profiles)?.auth ?? null);
}
