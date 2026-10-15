import type { Host, LoginProfile, ProfileWithUsage, SessionInfo } from "../ipc/api";

/** Complete host with deterministic ids (tests must not rely on `newId()`, which is automocked). */
export function makeHost(overrides: Partial<Host> = {}): Host {
  return {
    id: "h-web",
    name: "web-prod-01",
    address: "10.0.1.12",
    port: 22,
    user: "deploy",
    groupId: null,
    color: "accent",
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
    sftp: {
      remoteStart: null,
      localStart: null,
      transferMode: "binary",
      parallel: 4,
      uploadPerms: null,
      bandwidthLimitKbps: null,
      sftpServerCmd: null,
    },
    forwards: [],
    terminalScheme: null,
    terminalFontSize: null,
    source: "linascp",
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

export function makeProfile(overrides: Partial<LoginProfile> = {}): LoginProfile {
  return {
    id: "p-deploy",
    name: "deploy",
    user: "deploy",
    auth: { method: "key", keyPath: "~/.ssh/id_ed25519", hasSecret: false },
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

export function withUsage(profile: LoginProfile, usedBy: number): ProfileWithUsage {
  return { profile, usedBy };
}

export function makeSessionInfo(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    hostKeyAlgorithm: "ssh-ed25519",
    hostKeySha256: "SHA256:abc",
    kex: "curve25519-sha256",
    cipher: "chacha20-poly1305@openssh.com",
    mac: "none",
    authMethods: ["key"],
    ...overrides,
  };
}
