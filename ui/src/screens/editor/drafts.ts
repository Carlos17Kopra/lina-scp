import type {
  AuthConfig,
  AuthMethod,
  EnvVar,
  Host,
  HostAuth,
  HostSource,
  Id,
  LoginProfile,
  PortForward,
  TransferMode,
} from "../../ipc/api";
import { nowSeconds } from "../../model/defaults";
import { type HostColor, isHostColor } from "../../model/hostFormat";

export type EditorTab = "general" | "auth" | "connection" | "sftp";
export type AuthSource = "profile" | "custom";

/** Select values: no group / create a new group / no jump host. */
export const NO_GROUP = "";
export const NEW_GROUP = "__new__";
export const NO_JUMP = "";

/** Form state of credentials; secrets are only sent to the keyring after saving and never kept in stores. */
export interface AuthDraft {
  method: AuthMethod;
  keyPath: string;
  password: string;
  passphrase: string;
  /** A secret is already stored in the keyring. */
  hasSecret: boolean;
}

export interface SftpDraft {
  remoteStart: string;
  localStart: string;
  transferMode: TransferMode;
  parallel: string;
  uploadPerms: string;
  bandwidthLimit: string;
  sftpServerCmd: string;
}

/** Editor form state of a host (numbers as strings for inline validation). */
export interface HostDraft {
  id: Id;
  source: HostSource;
  createdAt: number;
  /** Terminal overrides are edited in the terminal sidebar, not here; carried through unchanged. */
  terminalScheme: string | null;
  terminalFontSize: number | null;
  name: string;
  address: string;
  port: string;
  groupId: string;
  newGroupName: string;
  tags: string;
  protocolSsh: boolean;
  protocolSftp: boolean;
  color: HostColor;
  notes: string;
  authSource: AuthSource;
  profileId: string;
  user: string;
  auth: AuthDraft;
  jumpHostId: string;
  keepalive: string;
  timeout: string;
  startCommand: string;
  env: string;
  forwards: PortForward[];
  sftp: SftpDraft;
}

export interface ProfileDraft {
  id: Id;
  createdAt: number;
  name: string;
  user: string;
  auth: AuthDraft;
}

const optional = (text: string): string | null => text.trim() || null;

function authToDraft(auth: AuthConfig): AuthDraft {
  return { method: auth.method, keyPath: auth.keyPath ?? "", password: "", passphrase: "", hasSecret: auth.hasSecret };
}

export function draftToAuthConfig(auth: AuthDraft): AuthConfig {
  const entered = (auth.method === "password" && auth.password !== "") || (auth.method === "key" && auth.passphrase !== "");
  return {
    method: auth.method,
    keyPath: auth.method === "key" ? optional(auth.keyPath) : null,
    hasSecret: auth.hasSecret || entered,
  };
}

export function parseTags(text: string): string[] {
  return [...new Set(text.split(",").map((tag) => tag.trim()).filter((tag) => tag.length > 0))];
}

/** One `KEY=VALUE` per line; blank lines are ignored (validation rejects other lines before saving). */
export function parseEnv(text: string): EnvVar[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const eq = line.indexOf("=");
      return eq === -1 ? { key: line, value: "" } : { key: line.slice(0, eq).trim(), value: line.slice(eq + 1) };
    });
}

export function formatEnv(env: readonly EnvVar[]): string {
  return env.map((entry) => `${entry.key}=${entry.value}`).join("\n");
}

export function hostToDraft(host: Host): HostDraft {
  const auth = host.auth;
  return {
    id: host.id,
    source: host.source,
    createdAt: host.createdAt,
    terminalScheme: host.terminalScheme ?? null,
    terminalFontSize: host.terminalFontSize ?? null,
    name: host.name,
    address: host.address,
    port: String(host.port),
    groupId: host.groupId ?? NO_GROUP,
    newGroupName: "",
    tags: host.tags.join(", "),
    protocolSsh: host.protocolSsh,
    protocolSftp: host.protocolSftp,
    color: isHostColor(host.color) ? host.color : "accent",
    notes: host.notes,
    authSource: auth.kind === "profile" ? "profile" : "custom",
    profileId: auth.kind === "profile" ? auth.profileId : "",
    user: host.user ?? "",
    auth: authToDraft(auth.kind === "custom" ? auth.auth : { method: "password", keyPath: null, hasSecret: false }),
    jumpHostId: host.jumpHostId ?? NO_JUMP,
    keepalive: String(host.keepaliveS),
    timeout: String(host.timeoutS),
    startCommand: host.startCommand ?? "",
    env: formatEnv(host.env),
    forwards: host.forwards,
    sftp: {
      remoteStart: host.sftp.remoteStart ?? "",
      localStart: host.sftp.localStart ?? "",
      transferMode: host.sftp.transferMode,
      parallel: String(host.sftp.parallel),
      uploadPerms: host.sftp.uploadPerms ?? "",
      bandwidthLimit: host.sftp.bandwidthLimitKbps === null ? "" : String(host.sftp.bandwidthLimitKbps),
      sftpServerCmd: host.sftp.sftpServerCmd ?? "",
    },
  };
}

/** Group id of an existing group, `null` for none or a group that still has to be created. */
export function draftGroupId(draft: HostDraft): Id | null {
  return draft.groupId === NO_GROUP || draft.groupId === NEW_GROUP ? null : draft.groupId;
}

/** Builds the host to save (always `source: "linascp"`); `groupId` is resolved by the caller. */
export function draftToHost(draft: HostDraft, groupId: Id | null): Host {
  const auth: HostAuth =
    draft.authSource === "profile"
      ? { kind: "profile", profileId: draft.profileId }
      : { kind: "custom", auth: draftToAuthConfig(draft.auth) };
  return {
    id: draft.id,
    name: draft.name.trim(),
    address: draft.address.trim(),
    port: Number(draft.port.trim()),
    user: draft.authSource === "custom" ? optional(draft.user) : null,
    groupId,
    color: draft.color,
    notes: draft.notes,
    tags: parseTags(draft.tags),
    protocolSsh: draft.protocolSsh,
    protocolSftp: draft.protocolSftp,
    auth,
    jumpHostId: draft.jumpHostId || null,
    keepaliveS: Number(draft.keepalive.trim()),
    timeoutS: Number(draft.timeout.trim()),
    startCommand: optional(draft.startCommand),
    env: parseEnv(draft.env),
    sftp: {
      remoteStart: optional(draft.sftp.remoteStart),
      localStart: optional(draft.sftp.localStart),
      transferMode: draft.sftp.transferMode,
      parallel: Number(draft.sftp.parallel.trim()),
      uploadPerms: optional(draft.sftp.uploadPerms),
      bandwidthLimitKbps: draft.sftp.bandwidthLimit.trim() ? Number(draft.sftp.bandwidthLimit.trim()) : null,
      sftpServerCmd: optional(draft.sftp.sftpServerCmd),
    },
    forwards: draft.forwards.map((forward) =>
      forward.kind === "dynamic" ? { ...forward, bind: forward.bind.trim(), target: "" } : { ...forward, bind: forward.bind.trim(), target: forward.target.trim() },
    ),
    terminalScheme: draft.terminalScheme,
    terminalFontSize: draft.terminalFontSize,
    source: "linascp",
    createdAt: draft.createdAt,
    updatedAt: nowSeconds(),
  };
}

export function profileToDraft(profile: LoginProfile): ProfileDraft {
  return { id: profile.id, createdAt: profile.createdAt, name: profile.name, user: profile.user, auth: authToDraft(profile.auth) };
}

export function draftToProfile(draft: ProfileDraft): LoginProfile {
  return {
    id: draft.id,
    name: draft.name.trim(),
    user: draft.user.trim(),
    auth: draftToAuthConfig(draft.auth),
    createdAt: draft.createdAt,
    updatedAt: nowSeconds(),
  };
}
