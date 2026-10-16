import { type Host, type LoginProfile, newId, type SftpSettings } from "../ipc/api";

export const DEFAULT_PORT = 22;
/** Tag of hosts created by Quick Connect (used for dedupe). */
export const QUICK_CONNECT_TAG = "quick-connect";

export function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

/** Mirrors core `SftpSettings::default()`. */
export function defaultSftpSettings(): SftpSettings {
  return {
    remoteStart: null,
    localStart: null,
    parallel: 4,
    uploadPerms: null,
    bandwidthLimitKbps: null,
    sftpServerCmd: null,
  };
}

/** Mirrors core `Host::new`: port 22, keepalive 30 s, timeout 15 s, SSH + SFTP, individual password auth. */
export function newHost(overrides: Partial<Host> = {}): Host {
  const now = nowSeconds();
  return {
    id: newId(),
    name: "",
    address: "",
    port: DEFAULT_PORT,
    user: null,
    groupId: null,
    color: null,
    notes: "",
    tags: [],
    protocolSsh: true,
    protocolSftp: true,
    auth: { kind: "custom", auth: { method: "password", keyPath: null, hasSecret: false } },
    jumpHostId: null,
    keepaliveS: 30,
    timeoutS: 15,
    startCommand: null,
    env: [],
    sftp: defaultSftpSettings(),
    forwards: [],
    terminalScheme: null,
    terminalFontSize: null,
    source: "linascp",
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

export function newProfile(overrides: Partial<LoginProfile> = {}): LoginProfile {
  const now = nowSeconds();
  return {
    id: newId(),
    name: "",
    user: "",
    auth: { method: "key", keyPath: null, hasSecret: false },
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}
