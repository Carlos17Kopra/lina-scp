# Part 40 — UI screens and packaging (Tasks 19–24)

Conventions used in this part (same as part 30, plus one addition):

- A bold path line (e.g. **`ui/src/screens/hosts/HostCard.tsx`**) directly above a code block means: write the **complete** file with exactly this content (create it, or replace it entirely if it already exists).
- **`ui/src/i18n/de.ts`** (insert before `} as const;`) means: paste the block into `ui/src/i18n/de.ts` directly above the final `} as const;` line (i.e. as new top-level groups of `t`). Helper functions that a block needs are declared in the same block **above** `export const t` only when stated; all interpolating strings are arrow functions inside `t`.
- All commands run from the repository root (`bun run test <filter>` runs Vitest with a file-name filter).
- Tests mock IPC with `vi.mock("<relative>/ipc/api")` (automock). Automocked `newId()` returns `undefined`, so tests that create entities set `vi.mocked(api.newId).mockReturnValue(...)`. Event channels are captured with `vi.mocked(api.createEventChannel).mockImplementation(...)`.
- Verification: all UI code of this part was assembled on top of part 30 in a scratch copy and checked with Vitest 4.1.11 / jsdom 29.1.1 (Node 20.19), `tsc` 7 and `vite build` (results at the end of Task 24).
- Decisions taken in this part (binding for later parts):
  1. **Quick Connect saves a host.** `connect_ssh` only accepts a stored `host_id`, so Quick Connect creates (once) a LinaSCP host named like the input (`user@host[:port]`), tagged `quick-connect`, SSH only, individual password auth (password is asked inline during connect), and then connects. A second Quick Connect to the same `user@host:port` reuses it (dedupe by tag + user + address + port).
  2. **No fingerprint lookup in the detail panel** (no command exists) → shows "—". No "zuletzt verbunden"/status dot (no data in TP1). The sidebar "Sync"/vault card is omitted (not a TP1 feature).
  3. **Editor fields without a model field are omitted:** SCP (Fallback), Terminal-Typ (always `xterm-256color`), Kompression/X11/Logging, Agent-Forwarding, the SFTP checkboxes, key generation, public key display, ssh-copy-id. Groups can be created inline ("+ Neue Gruppe …").
  4. **Editor navigation:** a stack of frames (`host` → `profile`); the Host/Login-Profil switch of the mockup is replaced by "Profil bearbeiten"/"Neues Login-Profil" (push) and "Zurück zum Host" (pop, restores the unsaved host draft). A saved pushed profile is selected in the host draft.
  5. **Connect screens are a full-height overlay right of the rail** (`ModalLayer` type `connect`); the rail highlights the originating protocol by switching the view (`terminal` for SSH) when a flow starts. Host-key prompts render the HostKeyNew/HostKeyChanged screens, other prompts render inline in the step list. The flow outcome comes from the `connectSsh` promise; events only drive progress.
  6. **Retry/backoff:** `attempt` starts at 1; the label is `Versuch min(attempt, 3) von 3`; auto-retry delays `nextRetryDelay(attempt)` = 2 s / 4 s / 8 s for attempts 1–3, `null` afterwards (stop). Manual "Erneut versuchen" is always possible and continues the counter.
  7. **Notices:** stores cannot use the React toast context; they push to `useNoticeStore`, which `AppShell` drains into toasts (`useNoticeToasts`).
  8. **Clipboard:** WebKitGTK exposes no PRIMARY selection to web content; selecting text copies it to the regular clipboard (`navigator.clipboard`), middle click pastes from it, `Ctrl+Shift+C/V` copy/paste.
  9. **Desktop entry:** a custom `.desktop` Handlebars template (`src-tauri/linascp.desktop`) sets `Categories=Network;RemoteAccess;` (Tauri's `category` only allows one value); the AppImage reuses the deb data dir and therefore the same desktop file.

---

### Task 19: Host overview (HostsScreen)

**Files:**
- Create: `ui/src/model/defaults.ts`, `ui/src/model/hostFormat.ts`, `ui/src/test/fixtures.ts`
- Create: `ui/src/screens/hosts/quickConnect.ts`, `ui/src/screens/hosts/filterHosts.ts`, `ui/src/screens/hosts/useHostActions.ts`
- Create: `ui/src/screens/hosts/HostsSidebar.tsx` + `.module.css`, `HostsToolbar.tsx` + `.module.css`, `HostGrid.tsx` + `.module.css`, `HostCard.tsx` + `.module.css`, `HostDetail.tsx` + `.module.css`, `HostsScreen.module.css`
- Modify (full replacement of the stub): `ui/src/screens/hosts/HostsScreen.tsx`
- Modify: `ui/src/i18n/de.ts` (add `t.auth`, `t.hosts`)
- Test: `ui/src/model/hostFormat.test.ts`, `ui/src/screens/hosts/quickConnect.test.ts`, `ui/src/screens/hosts/filterHosts.test.ts`, `ui/src/screens/hosts/HostsScreen.test.tsx`

**Interfaces:**
- Consumes: `useHostsStore` (`hosts, groups, profiles, filter, query, groupId, selectedId, setFilter, setQuery, setGroupId, select, saveHost, deleteHost, reloadSshConfig`), `useUiStore` (`openModal, searchFocusToken`), `useConnectStore.start(hostId, "ssh")` (stub until Task 21), component kit, `ipc/errors.errorDetails`.
- Produces:
  - `model/defaults.ts`: `DEFAULT_PORT = 22`, `QUICK_CONNECT_TAG = "quick-connect"`, `defaultSftpSettings(): SftpSettings`, `newHost(overrides?: Partial<Host>): Host`, `newProfile(overrides?: Partial<LoginProfile>): LoginProfile`, `nowSeconds(): number`.
  - `model/hostFormat.ts`: `HOST_COLORS`, `type HostColor`, `isHostColor(v)`, `hostColorVar(color: string | null): string`, `initials(name)`, `formatEndpoint(user: string | null | undefined, address, port, alwaysPort?)`, `formatAddress(host)`, `methodLabel(method: AuthMethod)`, `hostAuthConfig(host, profiles): AuthConfig | null`, `hostProfile(host, profiles): LoginProfile | null`.
  - `screens/hosts/quickConnect.ts`: `type QuickTarget = { user?: string; host: string; port: number }`, `parseQuickConnect(input): QuickTarget | null`, `formatQuickTarget(target)`, `findQuickConnectHost(hosts, target)`, `quickConnectHost(target): Host`.
  - `screens/hosts/filterHosts.ts`: `matchesQuery(host, query)`, `filterHosts(hosts, { filter, query, groupId })`.
  - `screens/hosts/useHostActions.ts`: `useHostActions(): { connect(host), edit(host), remove(host): Promise<void> }`, `startQuickConnect(target): Promise<void>`.
  - `test/fixtures.ts`: `makeHost(overrides?)`, `makeProfile(overrides?)`, `withUsage(profile, usedBy)`.
  - `HostsScreen()` (same export as the stub; renders a visually hidden `h1` "Server-Übersicht").

- [ ] **Step 1: Write the failing tests for the pure helpers**

**`ui/src/test/fixtures.ts`**
```ts
import type { Host, LoginProfile, ProfileWithUsage } from "../ipc/api";

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
```

**`ui/src/model/hostFormat.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { makeHost, makeProfile, withUsage } from "../test/fixtures";
import { formatAddress, formatEndpoint, hostAuthConfig, hostColorVar, hostProfile, initials, methodLabel } from "./hostFormat";

describe("initials", () => {
  it("uses the first letters of dash/dot separated parts", () => {
    expect(initials("web-prod-01")).toBe("WP");
    expect(initials("kunde.shop")).toBe("KS");
  });

  it("uses the first two letters of a single word", () => {
    expect(initials("bastion")).toBe("BA");
    expect(initials("x")).toBe("X");
    expect(initials("")).toBe("?");
  });
});

describe("formatAddress / formatEndpoint", () => {
  it("omits the default port", () => {
    expect(formatAddress(makeHost())).toBe("deploy@10.0.1.12");
    expect(formatAddress(makeHost({ port: 2222, user: null }))).toBe("10.0.1.12:2222");
  });

  it("brackets IPv6 addresses when a port is shown", () => {
    expect(formatEndpoint("root", "::1", 2222)).toBe("root@[::1]:2222");
    expect(formatEndpoint(null, "::1", 22)).toBe("::1");
    expect(formatEndpoint("deploy", "10.0.1.12", 22, true)).toBe("deploy@10.0.1.12:22");
  });
});

describe("hostColorVar", () => {
  it("maps known colors to tokens and falls back to the accent", () => {
    expect(hostColorVar("violet")).toBe("var(--violet)");
    expect(hostColorVar(null)).toBe("var(--accent)");
    expect(hostColorVar("#ff0000")).toBe("var(--accent)");
  });
});

describe("auth helpers", () => {
  const profile = makeProfile({ auth: { method: "agent", keyPath: null, hasSecret: false } });
  const profiles = [withUsage(profile, 2)];

  it("resolves the effective auth config and profile", () => {
    const viaProfile = makeHost({ auth: { kind: "profile", profileId: profile.id } });
    expect(hostProfile(viaProfile, profiles)).toBe(profile);
    expect(hostAuthConfig(viaProfile, profiles)?.method).toBe("agent");
    expect(hostAuthConfig(makeHost(), profiles)?.method).toBe("password");
    expect(hostProfile(makeHost(), profiles)).toBeNull();
    expect(hostAuthConfig(makeHost({ auth: { kind: "profile", profileId: "gone" } }), profiles)).toBeNull();
  });

  it("labels auth methods in German", () => {
    expect(methodLabel("keyboardInteractive")).toBe("Interaktiv / 2FA");
    expect(methodLabel("key")).toBe("Schlüssel");
  });
});
```

**`ui/src/screens/hosts/quickConnect.test.ts`**
```ts
import { describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { makeHost } from "../../test/fixtures";
import { findQuickConnectHost, formatQuickTarget, parseQuickConnect, quickConnectHost } from "./quickConnect";

vi.mock("../../ipc/api");

describe("parseQuickConnect", () => {
  it("parses user, host and port", () => {
    expect(parseQuickConnect("deploy@10.0.1.12:2222")).toEqual({ user: "deploy", host: "10.0.1.12", port: 2222 });
    expect(parseQuickConnect("  example.com  ")).toEqual({ host: "example.com", port: 22 });
    expect(parseQuickConnect("root@nas.local")).toEqual({ user: "root", host: "nas.local", port: 22 });
  });

  it("parses IPv6 addresses with and without brackets", () => {
    expect(parseQuickConnect("[::1]:2222")).toEqual({ host: "::1", port: 2222 });
    expect(parseQuickConnect("root@[fe80::1%eth0]")).toEqual({ user: "root", host: "fe80::1%eth0", port: 22 });
    expect(parseQuickConnect("::1")).toEqual({ host: "::1", port: 22 });
    expect(parseQuickConnect("admin@2001:db8::10")).toEqual({ user: "admin", host: "2001:db8::10", port: 22 });
  });

  it("rejects malformed input", () => {
    for (const input of ["", "   ", "user@", "@host", "host:", "host:0", "host:65536", "host:22a", "[::1", "[::1]x", "[]", "a b", "us er@host", "[example.com]:22", "host/path"]) {
      expect(parseQuickConnect(input), input).toBeNull();
    }
  });
});

describe("quick connect hosts", () => {
  it("formats targets like the input", () => {
    expect(formatQuickTarget({ user: "deploy", host: "10.0.1.12", port: 22 })).toBe("deploy@10.0.1.12");
    expect(formatQuickTarget({ host: "::1", port: 2222 })).toBe("[::1]:2222");
  });

  it("builds an SSH-only host tagged quick-connect", () => {
    vi.mocked(api.newId).mockReturnValue("q-1");
    const host = quickConnectHost({ user: "deploy", host: "10.0.0.5", port: 2222 });
    expect(host).toMatchObject({
      id: "q-1",
      name: "deploy@10.0.0.5:2222",
      address: "10.0.0.5",
      port: 2222,
      user: "deploy",
      tags: ["quick-connect"],
      protocolSsh: true,
      protocolSftp: false,
      source: "linascp",
      auth: { kind: "custom", auth: { method: "password", keyPath: null, hasSecret: false } },
    });
  });

  it("finds an existing quick-connect host by user, address and port", () => {
    const existing = makeHost({ id: "q-1", address: "10.0.0.5", port: 2222, user: "deploy", tags: ["quick-connect"] });
    const manual = makeHost({ id: "m-1", address: "10.0.0.5", port: 2222, user: "deploy" });
    expect(findQuickConnectHost([manual, existing], { user: "deploy", host: "10.0.0.5", port: 2222 })).toBe(existing);
    expect(findQuickConnectHost([existing], { host: "10.0.0.5", port: 2222 })).toBeUndefined();
    expect(findQuickConnectHost([existing], { user: "deploy", host: "10.0.0.5", port: 22 })).toBeUndefined();
  });
});
```

**`ui/src/screens/hosts/filterHosts.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { makeHost } from "../../test/fixtures";
import { filterHosts, matchesQuery } from "./filterHosts";

const web = makeHost({ id: "web", name: "web-prod-01", tags: ["nginx", "docker"], groupId: "g-prod" });
const db = makeHost({ id: "db", name: "db-prod-01", address: "10.0.1.20", user: "postgres", protocolSftp: false, groupId: "g-prod" });
const shop = makeHost({ id: "shop", name: "kunde-shop", address: "sftp.kunde-a.example", user: "shop", protocolSsh: false });
const all = [web, db, shop];
const ids = (hosts: { id: string }[]) => hosts.map((h) => h.id);

describe("matchesQuery", () => {
  it("matches name, address, user and tags case-insensitively", () => {
    expect(matchesQuery(web, "WEB")).toBe(true);
    expect(matchesQuery(db, "10.0.1.20")).toBe(true);
    expect(matchesQuery(db, "postgres")).toBe(true);
    expect(matchesQuery(web, "docker")).toBe(true);
    expect(matchesQuery(web, "")).toBe(true);
  });

  it("requires every whitespace separated term to match", () => {
    expect(matchesQuery(web, "prod nginx")).toBe(true);
    expect(matchesQuery(web, "prod postgres")).toBe(false);
  });
});

describe("filterHosts", () => {
  it("filters by protocol", () => {
    expect(ids(filterHosts(all, { filter: "all", query: "", groupId: null }))).toEqual(["web", "db", "shop"]);
    expect(ids(filterHosts(all, { filter: "ssh", query: "", groupId: null }))).toEqual(["web", "db"]);
    expect(ids(filterHosts(all, { filter: "sftp", query: "", groupId: null }))).toEqual(["web", "shop"]);
  });

  it("combines group, protocol and query", () => {
    expect(ids(filterHosts(all, { filter: "all", query: "", groupId: "g-prod" }))).toEqual(["web", "db"]);
    expect(ids(filterHosts(all, { filter: "sftp", query: "", groupId: "g-prod" }))).toEqual(["web"]);
    expect(ids(filterHosts(all, { filter: "all", query: "kunde", groupId: null }))).toEqual(["shop"]);
  });
});
```

- [ ] **Step 2: Run the helper tests to verify they fail**

Run: `bun run test hostFormat quickConnect filterHosts`
Expected: FAIL — `Failed to resolve import "./hostFormat"`, `"./quickConnect"`, `"./filterHosts"`.

- [ ] **Step 3: Implement the helpers and the German strings**

**`ui/src/model/defaults.ts`**
```ts
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
    transferMode: "binary",
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
```

**`ui/src/model/hostFormat.ts`**
```ts
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
```

**`ui/src/screens/hosts/quickConnect.ts`**
```ts
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
```

**`ui/src/screens/hosts/filterHosts.ts`**
```ts
import type { Host, Id } from "../../ipc/api";
import type { ProtocolFilter } from "../../stores/hosts";

export interface HostFilter {
  filter: ProtocolFilter;
  query: string;
  /** `null` = all groups. */
  groupId: Id | null;
}

/** Every whitespace separated term must occur in name, address, user or a tag (case-insensitive). */
export function matchesQuery(host: Host, query: string): boolean {
  const terms = query.toLowerCase().split(/\s+/).filter((term) => term.length > 0);
  if (terms.length === 0) return true;
  const fields = [host.name, host.address, host.user ?? "", ...host.tags].map((field) => field.toLowerCase());
  return terms.every((term) => fields.some((field) => field.includes(term)));
}

export function filterHosts(hosts: readonly Host[], { filter, query, groupId }: HostFilter): Host[] {
  return hosts.filter(
    (host) =>
      (filter === "all" || (filter === "ssh" ? host.protocolSsh : host.protocolSftp)) &&
      (groupId === null || host.groupId === groupId) &&
      matchesQuery(host, query),
  );
}
```

**`ui/src/i18n/de.ts`** (insert before `} as const;`)
```ts
  auth: {
    password: "Passwort",
    key: "Schlüssel",
    agent: "SSH-Agent",
    keyboardInteractive: "Interaktiv / 2FA",
  },
  hosts: {
    title: "Server-Übersicht",
    sidebar: "Gruppen und Login-Profile",
    groups: "Gruppen",
    allHosts: "Alle Hosts",
    noGroup: "Ohne Gruppe",
    profiles: "Login-Profile",
    addProfile: "Login-Profil hinzufügen",
    noProfiles: "Noch keine Login-Profile",
    profileMeta: (method: string, usedBy: number) => `${method} · ${usedBy === 1 ? "1 Host" : `${usedBy} Hosts`}`,
    search: "Hosts, Tags oder IP durchsuchen",
    searchShortcut: "Ctrl+K",
    quickConnectLabel: "Quick Connect",
    quickConnect: "Quick Connect: user@host:port",
    quickConnectInvalid: "Format: user@host:port",
    newHost: "Neuer Host",
    newHostTitle: "Neuer Host (Ctrl+N)",
    reloadSshConfig: "ssh-config neu einlesen",
    sshConfigReloaded: "~/.ssh/config wurde neu eingelesen",
    filter: "Protokoll filtern",
    filterAll: "Alle",
    filterSsh: "SSH",
    filterSftp: "SFTP",
    count: (n: number) => (n === 1 ? "1 Host" : `${n} Hosts`),
    fromSshConfig: "aus ssh-config",
    select: (name: string) => `${name} auswählen`,
    openTerminal: "Terminal öffnen",
    openFiles: "Dateien öffnen",
    filesLater: "Folgt in Teilprojekt 3",
    edit: "Host bearbeiten",
    emptyTitle: "Noch keine Hosts",
    emptyText: "Lege einen Host an oder lies deine ~/.ssh/config neu ein.",
    noMatchTitle: "Keine Hosts gefunden",
    noMatchText: "Passe Suche, Gruppe oder Protokollfilter an.",
    details: "Host-Details",
    noSelection: "Wähle einen Host aus, um Details zu sehen.",
    terminal: "Terminal",
    files: "Dateien",
    address: "Adresse",
    port: "Port",
    profile: "Login-Profil",
    jumpHost: "Jump-Host",
    startPath: "Startpfad",
    fingerprint: "Fingerprint",
    customAuth: (method: string) => `Individuell · ${method}`,
    none: "—",
    portForwarding: "Port-Forwarding",
    noRules: "Keine Regeln",
    socks: (bind: string) => `SOCKS ${bind}`,
    forwardKind: { local: "L", remote: "R", dynamic: "D" },
    delete: "Löschen",
    deleteTitle: "Host löschen?",
    deleteText: (name: string) =>
      `»${name}« wird endgültig gelöscht. Gespeicherte Passwörter und Passphrasen werden aus dem Schlüsselbund entfernt.`,
    deleted: (name: string) => `»${name}« wurde gelöscht`,
    deleteFailed: "Löschen fehlgeschlagen",
  },
```

Run: `bun run test hostFormat quickConnect filterHosts`
Expected: PASS (hostFormat 7, quickConnect 6, filterHosts 4).

- [ ] **Step 4: Write the failing HostsScreen tests**

**`ui/src/screens/hosts/HostsScreen.test.tsx`**
```tsx
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { makeHost, makeProfile, withUsage } from "../../test/fixtures";
import { HostsScreen } from "./HostsScreen";

vi.mock("../../ipc/api");

const deploy = makeProfile();
const web = makeHost({
  id: "h-web",
  name: "web-prod-01",
  groupId: "g-prod",
  tags: ["nginx"],
  auth: { kind: "profile", profileId: deploy.id },
  jumpHostId: "h-bastion",
  sftp: { remoteStart: "/var/www", localStart: null, transferMode: "binary", parallel: 4, uploadPerms: null, bandwidthLimitKbps: null, sftpServerCmd: null },
  forwards: [{ id: "f1", kind: "local", bind: "8080", target: "localhost:80", autostart: true }],
});
const bastion = makeHost({ id: "h-bastion", name: "bastion", address: "bastion.example.net", port: 2222, user: "admin", protocolSftp: false, source: "sshConfig", groupId: "g-infra" });
const shop = makeHost({ id: "h-shop", name: "kunde-shop", address: "sftp.kunde-a.example", user: "shop", protocolSsh: false });
const groups = [
  { id: "g-prod", name: "Produktion", sortOrder: 0 },
  { id: "g-infra", name: "Infrastruktur", sortOrder: 1 },
];
const start = vi.fn(async () => undefined);

function seed(hosts = [web, bastion, shop]) {
  useHostsStore.setState({ hosts, groups, profiles: [withUsage(deploy, 1)], loaded: true });
  vi.mocked(api.hostsList).mockResolvedValue(hosts);
  vi.mocked(api.groupsList).mockResolvedValue(groups);
  vi.mocked(api.profilesList).mockResolvedValue([withUsage(deploy, 1)]);
}

function renderScreen() {
  return render(
    <ToastProvider>
      <HostsScreen />
    </ToastProvider>,
  );
}

const cardOf = (name: string) => screen.getByRole("button", { name: `${name} auswählen` }).closest("li") as HTMLElement;

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  seed();
});

describe("HostsScreen", () => {
  it("lists host cards with protocol chips, tags and the ssh-config badge", () => {
    renderScreen();
    expect(screen.getByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Alle Hosts" })).toBeInTheDocument();
    expect(screen.getByText("3 Hosts")).toBeInTheDocument();
    const webCard = cardOf("web-prod-01");
    expect(within(webCard).getByText("deploy@10.0.1.12")).toBeInTheDocument();
    expect(within(webCard).getByText("SSH")).toBeInTheDocument();
    expect(within(webCard).getByText("SFTP")).toBeInTheDocument();
    expect(within(webCard).getByText("nginx")).toBeInTheDocument();
    expect(within(cardOf("bastion")).getByText("admin@bastion.example.net:2222")).toBeInTheDocument();
    expect(within(cardOf("bastion")).getByText("aus ssh-config")).toBeInTheDocument();
    expect(within(cardOf("kunde-shop")).queryByRole("button", { name: "Terminal öffnen" })).not.toBeInTheDocument();
  });

  it("filters by protocol and search text", async () => {
    renderScreen();
    await userEvent.click(screen.getByRole("radio", { name: "SFTP" }));
    expect(screen.queryByRole("button", { name: "bastion auswählen" })).not.toBeInTheDocument();
    expect(screen.getByText("2 Hosts")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: "Alle" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Hosts, Tags oder IP durchsuchen" }), "nginx");
    expect(screen.getByText("1 Host")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "web-prod-01 auswählen" })).toBeInTheDocument();
    await userEvent.clear(screen.getByRole("searchbox"));
    await userEvent.type(screen.getByRole("searchbox"), "gibt-es-nicht");
    expect(screen.getByText("Keine Hosts gefunden")).toBeInTheDocument();
  });

  it("filters by group and lists login profiles with their usage", async () => {
    renderScreen();
    const sidebar = screen.getByRole("complementary", { name: "Gruppen und Login-Profile" });
    await userEvent.click(within(sidebar).getByRole("button", { name: /Infrastruktur/ }));
    expect(screen.getByRole("heading", { name: "Infrastruktur" })).toBeInTheDocument();
    expect(screen.getByText("1 Host")).toBeInTheDocument();
    expect(within(sidebar).getByText("Schlüssel · 1 Host")).toBeInTheDocument();
    await userEvent.click(within(sidebar).getByRole("button", { name: /deploy/ }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "profile", id: "p-deploy" } });
    await userEvent.click(within(sidebar).getByRole("button", { name: "Login-Profil hinzufügen" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "profile" } });
  });

  it("focuses the search when the focus token changes (Ctrl+K)", () => {
    renderScreen();
    act(() => useUiStore.getState().focusSearch());
    expect(screen.getByRole("searchbox")).toHaveFocus();
  });

  it("shows the details of the selected host", async () => {
    renderScreen();
    expect(screen.getByText("Wähle einen Host aus, um Details zu sehen.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "web-prod-01 auswählen" }));
    const details = screen.getByRole("complementary", { name: "Host-Details" });
    expect(within(details).getByRole("heading", { name: "web-prod-01" })).toBeInTheDocument();
    expect(within(details).getByText("Produktion")).toBeInTheDocument();
    expect(within(details).getByText("deploy")).toBeInTheDocument();
    expect(within(details).getByText("bastion")).toBeInTheDocument();
    expect(within(details).getByText("/var/www")).toBeInTheDocument();
    expect(within(details).getByText("8080 → localhost:80")).toBeInTheDocument();
    const files = within(details).getByRole("button", { name: "Dateien" });
    expect(files).toHaveAttribute("aria-disabled", "true");
    expect(files).toHaveAttribute("title", "Folgt in Teilprojekt 3");
  });

  it("starts an SSH connection from the card and from the details", async () => {
    renderScreen();
    await userEvent.click(within(cardOf("web-prod-01")).getByRole("button", { name: "Terminal öffnen" }));
    expect(start).toHaveBeenCalledWith("h-web", "ssh");
    await userEvent.click(screen.getByRole("button", { name: "bastion auswählen" }));
    await userEvent.click(within(screen.getByRole("complementary", { name: "Host-Details" })).getByRole("button", { name: "Terminal" }));
    expect(start).toHaveBeenLastCalledWith("h-bastion", "ssh");
  });

  it("opens the editor for new and existing hosts", async () => {
    renderScreen();
    await userEvent.click(screen.getByRole("button", { name: "Neuer Host" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host" } });
    await userEvent.click(within(cardOf("kunde-shop")).getByRole("button", { name: "Host bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host", id: "h-shop" } });
  });

  it("deletes a host after confirmation, but never ssh-config hosts", async () => {
    vi.mocked(api.hostDelete).mockResolvedValue(undefined);
    renderScreen();
    await userEvent.click(screen.getByRole("button", { name: "bastion auswählen" }));
    expect(screen.queryByRole("button", { name: "Löschen" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "kunde-shop auswählen" }));
    await userEvent.click(screen.getByRole("button", { name: "Löschen" }));
    const confirm = screen.getByRole("alertdialog", { name: "Host löschen?" });
    expect(confirm).toHaveTextContent("»kunde-shop« wird endgültig gelöscht.");
    await userEvent.click(within(confirm).getByRole("button", { name: "Löschen" }));
    expect(api.hostDelete).toHaveBeenCalledWith("h-shop");
    expect(await screen.findByText("»kunde-shop« wurde gelöscht")).toBeInTheDocument();
  });

  it("Quick Connect saves a quick-connect host once and connects", async () => {
    vi.mocked(api.newId).mockReturnValue("q-1");
    vi.mocked(api.hostSave).mockImplementation(async (host) => host);
    renderScreen();
    const quick = screen.getByRole("textbox", { name: "Quick Connect" });
    await userEvent.type(quick, "deploy@10.0.0.5:2222{Enter}");
    await waitFor(() => expect(start).toHaveBeenCalledWith("q-1", "ssh"));
    expect(api.hostSave).toHaveBeenCalledWith(
      expect.objectContaining({ name: "deploy@10.0.0.5:2222", address: "10.0.0.5", port: 2222, user: "deploy", tags: ["quick-connect"] }),
    );
    expect(quick).toHaveValue("");

    const saved = makeHost({ id: "q-1", name: "deploy@10.0.0.5:2222", address: "10.0.0.5", port: 2222, user: "deploy", tags: ["quick-connect"] });
    act(() => useHostsStore.setState({ hosts: [web, saved] }));
    vi.mocked(api.hostSave).mockClear();
    await userEvent.type(quick, "deploy@10.0.0.5:2222{Enter}");
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
    expect(api.hostSave).not.toHaveBeenCalled();
  });

  it("rejects malformed Quick Connect input", async () => {
    renderScreen();
    const quick = screen.getByRole("textbox", { name: "Quick Connect" });
    await userEvent.type(quick, "user@{Enter}");
    expect(screen.getByText("Format: user@host:port")).toBeInTheDocument();
    expect(quick).toHaveAttribute("aria-invalid", "true");
    expect(start).not.toHaveBeenCalled();
  });

  it("re-reads the ssh-config", async () => {
    vi.mocked(api.sshConfigReload).mockResolvedValue([web]);
    renderScreen();
    await userEvent.click(screen.getByRole("button", { name: "ssh-config neu einlesen" }));
    expect(api.sshConfigReload).toHaveBeenCalledOnce();
    expect(await screen.findByText("~/.ssh/config wurde neu eingelesen")).toBeInTheDocument();
  });

  it("shows an empty state without hosts", async () => {
    seed([]);
    renderScreen();
    expect(screen.getByText("Noch keine Hosts")).toBeInTheDocument();
    expect(screen.getByText("0 Hosts")).toBeInTheDocument();
  });
});
```

- [ ] **Step 5: Run the screen tests to verify they fail**

Run: `bun run test HostsScreen`
Expected: FAIL — the stub renders only the placeholder heading (e.g. `Unable to find role="heading" and name "Alle Hosts"`).

- [ ] **Step 6: Implement the host overview**

**`ui/src/screens/hosts/useHostActions.ts`**
```ts
import { useMemo } from "react";
import { useToast } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { findQuickConnectHost, type QuickTarget, quickConnectHost } from "./quickConnect";

export interface HostActions {
  connect(host: Host): void;
  edit(host: Host): void;
  remove(host: Host): Promise<void>;
}

export function useHostActions(): HostActions {
  const toast = useToast();
  return useMemo(
    () => ({
      connect: (host) => {
        void useConnectStore.getState().start(host.id, "ssh");
      },
      edit: (host) => useUiStore.getState().openModal({ type: "editor", target: { kind: "host", id: host.id } }),
      remove: async (host) => {
        try {
          await useHostsStore.getState().deleteHost(host.id);
          toast.show({ tone: "neutral", message: t.hosts.deleted(host.name) });
        } catch (error) {
          toast.show({ tone: "red", message: t.hosts.deleteFailed, details: errorDetails(error) });
        }
      },
    }),
    [toast],
  );
}

/** Reuses the quick-connect host of the same `user@host:port` or saves a new one, then connects via SSH. */
export async function startQuickConnect(target: QuickTarget): Promise<void> {
  const hosts = useHostsStore.getState();
  const host = findQuickConnectHost(hosts.hosts, target) ?? (await hosts.saveHost(quickConnectHost(target)));
  await useConnectStore.getState().start(host.id, "ssh");
}
```

**`ui/src/screens/hosts/HostsScreen.tsx`**
```tsx
import { t } from "../../i18n/de";
import { HostDetail } from "./HostDetail";
import { HostGrid } from "./HostGrid";
import styles from "./HostsScreen.module.css";
import { HostsSidebar } from "./HostsSidebar";
import { HostsToolbar } from "./HostsToolbar";

/** Server overview: groups/profiles sidebar, toolbar (search, Quick Connect), host grid, detail panel. */
export function HostsScreen() {
  return (
    <div className={styles.screen}>
      <h1 className="visually-hidden">{t.hosts.title}</h1>
      <HostsSidebar />
      <div className={styles.main}>
        <HostsToolbar />
        <HostGrid />
      </div>
      <HostDetail />
    </div>
  );
}
```

**`ui/src/screens/hosts/HostsScreen.module.css`**
```css
.screen {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
  background: var(--bg);
}

.main {
  display: flex;
  flex: 999 1 560px;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}
```

**`ui/src/screens/hosts/HostsSidebar.tsx`**
```tsx
import { KeyRound, Plus } from "lucide-react";
import { useId } from "react";
import { cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { methodLabel } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import styles from "./HostsSidebar.module.css";

export function HostsSidebar() {
  const hosts = useHostsStore((state) => state.hosts);
  const groups = useHostsStore((state) => state.groups);
  const profiles = useHostsStore((state) => state.profiles);
  const groupId = useHostsStore((state) => state.groupId);
  const setGroupId = useHostsStore((state) => state.setGroupId);
  const openModal = useUiStore((state) => state.openModal);
  const groupsId = useId();
  const profilesId = useId();

  const items = [
    { id: null, name: t.hosts.allHosts, count: hosts.length },
    ...[...groups]
      .sort((a, b) => a.sortOrder - b.sortOrder || a.name.localeCompare(b.name))
      .map((group) => ({ id: group.id, name: group.name, count: hosts.filter((host) => host.groupId === group.id).length })),
  ];

  return (
    <aside className={styles.sidebar} aria-label={t.hosts.sidebar}>
      <section className={styles.section} aria-labelledby={groupsId}>
        <div className={styles.heading}>
          <h2 id={groupsId} className={styles.label}>
            {t.hosts.groups}
          </h2>
        </div>
        <ul className={styles.list}>
          {items.map((item) => {
            const active = item.id === groupId;
            return (
              <li key={item.id ?? "all"}>
                <button
                  type="button"
                  className={cx(styles.item, active && styles.active)}
                  aria-pressed={active}
                  onClick={() => setGroupId(item.id)}
                >
                  <span>{item.name}</span>
                  <span className={styles.count}>{item.count}</span>
                </button>
              </li>
            );
          })}
        </ul>
      </section>
      <section className={styles.section} aria-labelledby={profilesId}>
        <div className={styles.heading}>
          <h2 id={profilesId} className={styles.label}>
            {t.hosts.profiles}
          </h2>
          <IconButton
            label={t.hosts.addProfile}
            icon={Plus}
            size="sm"
            onClick={() => openModal({ type: "editor", target: { kind: "profile" } })}
          />
        </div>
        {profiles.length === 0 ? <p className={styles.empty}>{t.hosts.noProfiles}</p> : null}
        <ul className={styles.list}>
          {profiles.map(({ profile, usedBy }) => (
            <li key={profile.id}>
              <button
                type="button"
                className={cx(styles.item, styles.profile)}
                onClick={() => openModal({ type: "editor", target: { kind: "profile", id: profile.id } })}
              >
                <span className={styles.profileIcon} aria-hidden="true">
                  <KeyRound size={14} strokeWidth={2} />
                </span>
                <span className={styles.profileText}>
                  <span className={styles.profileName}>{profile.name}</span>
                  <span className={styles.profileMeta}>{t.hosts.profileMeta(methodLabel(profile.auth.method), usedBy)}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    </aside>
  );
}
```

**`ui/src/screens/hosts/HostsSidebar.module.css`**
```css
.sidebar {
  display: flex;
  flex: 0 0 232px;
  flex-direction: column;
  gap: 22px;
  padding: 20px 12px;
  overflow-y: auto;
  border-right: 1px solid var(--bd);
  background: var(--s1);
}

.section {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  padding: 0 4px 6px 10px;
}

.label {
  color: var(--muted);
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  padding: 9px 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text2);
  font-size: 14px;
  text-align: left;
  cursor: pointer;
}

.item:hover {
  background: var(--raised);
}

.active {
  background: var(--raised);
  color: var(--text);
  font-weight: 500;
}

.count {
  color: var(--muted);
  font-family: var(--font-mono);
  font-size: 12px;
}

.profile {
  justify-content: flex-start;
  color: var(--text);
}

.profileIcon {
  display: grid;
  flex: 0 0 26px;
  place-items: center;
  height: 26px;
  border-radius: 6px;
  background: var(--raised);
  color: var(--accent);
}

.profileText {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.profileName {
  overflow: hidden;
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.profileMeta {
  color: var(--muted);
  font-size: 12px;
}

.empty {
  padding: 0 10px;
  color: var(--muted);
  font-size: 13px;
}
```

**`ui/src/screens/hosts/HostsToolbar.tsx`**
```tsx
import { Plus, RefreshCw, Search, Zap } from "lucide-react";
import { type FormEvent, useEffect, useId, useRef, useState } from "react";
import { Button, cx, useToast } from "../../components";
import { t } from "../../i18n/de";
import { errorDetails } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import styles from "./HostsToolbar.module.css";
import { parseQuickConnect } from "./quickConnect";
import { startQuickConnect } from "./useHostActions";

export function HostsToolbar() {
  const query = useHostsStore((state) => state.query);
  const setQuery = useHostsStore((state) => state.setQuery);
  const reloadSshConfig = useHostsStore((state) => state.reloadSshConfig);
  const searchFocusToken = useUiStore((state) => state.searchFocusToken);
  const openModal = useUiStore((state) => state.openModal);
  const toast = useToast();
  const searchRef = useRef<HTMLInputElement>(null);
  const [quick, setQuick] = useState("");
  const [quickInvalid, setQuickInvalid] = useState(false);
  const quickErrorId = useId();

  useEffect(() => {
    if (searchFocusToken === 0) return;
    searchRef.current?.focus();
    searchRef.current?.select();
  }, [searchFocusToken]);

  const submitQuick = async (event: FormEvent) => {
    event.preventDefault();
    const target = parseQuickConnect(quick);
    if (!target) {
      setQuickInvalid(true);
      return;
    }
    setQuickInvalid(false);
    try {
      await startQuickConnect(target);
      setQuick("");
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    }
  };

  const reload = () => {
    reloadSshConfig().then(
      () => toast.show({ tone: "green", message: t.hosts.sshConfigReloaded }),
      (error: unknown) => toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) }),
    );
  };

  return (
    <header className={styles.toolbar}>
      <div className={cx(styles.field, styles.search)}>
        <Search size={16} aria-hidden="true" />
        <input
          ref={searchRef}
          type="search"
          className={styles.input}
          aria-label={t.hosts.search}
          placeholder={t.hosts.search}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <kbd className={styles.kbd}>{t.hosts.searchShortcut}</kbd>
      </div>
      <form className={cx(styles.field, styles.quick, quickInvalid && styles.invalid)} onSubmit={submitQuick}>
        <Zap size={16} aria-hidden="true" />
        <input
          className={cx(styles.input, "mono")}
          aria-label={t.hosts.quickConnectLabel}
          placeholder={t.hosts.quickConnect}
          value={quick}
          onChange={(event) => {
            setQuick(event.target.value);
            setQuickInvalid(false);
          }}
          aria-invalid={quickInvalid ? true : undefined}
          aria-describedby={quickInvalid ? quickErrorId : undefined}
          spellCheck={false}
          autoCapitalize="off"
        />
        {quickInvalid ? (
          <span id={quickErrorId} className={styles.error}>
            {t.hosts.quickConnectInvalid}
          </span>
        ) : null}
      </form>
      <Button icon={RefreshCw} onClick={reload}>
        {t.hosts.reloadSshConfig}
      </Button>
      <Button
        variant="primary"
        icon={Plus}
        title={t.hosts.newHostTitle}
        onClick={() => openModal({ type: "editor", target: { kind: "host" } })}
      >
        {t.hosts.newHost}
      </Button>
    </header>
  );
}
```

**`ui/src/screens/hosts/HostsToolbar.module.css`**
```css
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  padding: 16px 24px;
  border-bottom: 1px solid var(--bd);
  background: var(--s1);
}

.field {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 40px;
  padding: 0 12px;
  border: 1px solid var(--bd-strong);
  border-radius: 10px;
  background: var(--card);
  color: var(--muted);
}

.field:focus-within {
  border-color: var(--accent);
}

.search {
  flex: 1 1 260px;
}

.quick {
  flex: 1 1 240px;
}

.invalid,
.invalid:focus-within {
  border-color: var(--red);
}

.input {
  flex: 1;
  min-width: 0;
  height: 100%;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font-size: 14px;
}

.input::placeholder {
  color: var(--muted2);
}

.input::-webkit-search-cancel-button {
  display: none;
}

.kbd {
  padding: 1px 6px;
  border: 1px solid var(--bd-strong);
  border-radius: 6px;
  color: var(--muted);
  font-family: var(--font-mono);
  font-size: 11px;
}

.error {
  position: absolute;
  top: calc(100% + 4px);
  left: 12px;
  color: var(--red);
  font-size: 12px;
}
```

**`ui/src/screens/hosts/HostGrid.tsx`**
```tsx
import { Plus } from "lucide-react";
import { useId, useMemo } from "react";
import { Button, Segmented } from "../../components";
import { t } from "../../i18n/de";
import { type ProtocolFilter, useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { filterHosts } from "./filterHosts";
import { HostCard } from "./HostCard";
import styles from "./HostGrid.module.css";

const FILTER_OPTIONS: ReadonlyArray<{ value: ProtocolFilter; label: string }> = [
  { value: "all", label: t.hosts.filterAll },
  { value: "ssh", label: t.hosts.filterSsh },
  { value: "sftp", label: t.hosts.filterSftp },
];

export function HostGrid() {
  const hosts = useHostsStore((state) => state.hosts);
  const groups = useHostsStore((state) => state.groups);
  const filter = useHostsStore((state) => state.filter);
  const query = useHostsStore((state) => state.query);
  const groupId = useHostsStore((state) => state.groupId);
  const selectedId = useHostsStore((state) => state.selectedId);
  const setFilter = useHostsStore((state) => state.setFilter);
  const select = useHostsStore((state) => state.select);
  const openModal = useUiStore((state) => state.openModal);
  const titleId = useId();

  const visible = useMemo(() => filterHosts(hosts, { filter, query, groupId }), [hosts, filter, query, groupId]);
  const title = (groupId !== null && groups.find((group) => group.id === groupId)?.name) || t.hosts.allHosts;

  return (
    <section className={styles.grid} aria-labelledby={titleId}>
      <div className={styles.head}>
        <div>
          <h2 id={titleId} className={styles.title}>
            {title}
          </h2>
          <p className={styles.count}>{t.hosts.count(visible.length)}</p>
        </div>
        <Segmented label={t.hosts.filter} value={filter} options={FILTER_OPTIONS} onChange={setFilter} size="sm" />
      </div>
      {hosts.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t.hosts.emptyTitle}</p>
          <p>{t.hosts.emptyText}</p>
          <Button variant="primary" icon={Plus} onClick={() => openModal({ type: "editor", target: { kind: "host" } })}>
            {t.hosts.newHost}
          </Button>
        </div>
      ) : visible.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t.hosts.noMatchTitle}</p>
          <p>{t.hosts.noMatchText}</p>
        </div>
      ) : (
        <ul className={styles.cards}>
          {visible.map((host) => (
            <li key={host.id}>
              <HostCard host={host} selected={host.id === selectedId} onSelect={() => select(host.id)} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
```

**`ui/src/screens/hosts/HostGrid.module.css`**
```css
.grid {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 18px;
  min-height: 0;
  padding: 24px;
  overflow-y: auto;
}

.head {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  justify-content: space-between;
  gap: 12px;
}

.title {
  font-size: 20px;
  font-weight: 600;
}

.count {
  margin-top: 2px;
  color: var(--muted);
  font-size: 13px;
}

.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
  gap: 14px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  max-width: 380px;
  margin: 60px auto;
  color: var(--muted);
  text-align: center;
}

.emptyTitle {
  color: var(--text);
  font-size: 16px;
  font-weight: 600;
}
```

**`ui/src/screens/hosts/HostCard.tsx`**
```tsx
import { FolderOpen, Pencil, SquareTerminal } from "lucide-react";
import { Card, Chip, IconButton } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { formatAddress, hostColorVar, initials } from "../../model/hostFormat";
import styles from "./HostCard.module.css";
import { useHostActions } from "./useHostActions";

export function HostCard({ host, selected, onSelect }: { host: Host; selected: boolean; onSelect: () => void }) {
  const actions = useHostActions();
  return (
    <Card selected={selected} className={styles.card}>
      <button
        type="button"
        className={styles.select}
        aria-pressed={selected}
        aria-label={t.hosts.select(host.name)}
        onClick={onSelect}
      >
        <span className={styles.avatar} style={{ color: hostColorVar(host.color) }} aria-hidden="true">
          {initials(host.name)}
        </span>
        <span className={styles.titles}>
          <span className={styles.name}>{host.name}</span>
          <span className={styles.address}>{formatAddress(host)}</span>
        </span>
      </button>
      <div className={styles.chips}>
        {host.protocolSsh ? <Chip tone="accent">SSH</Chip> : null}
        {host.protocolSftp ? <Chip tone="amber">SFTP</Chip> : null}
        {host.source === "sshConfig" ? <Chip tone="neutral">{t.hosts.fromSshConfig}</Chip> : null}
        {host.tags.map((tag) => (
          <Chip key={tag} tone="neutral">
            {tag}
          </Chip>
        ))}
      </div>
      <div className={styles.footer}>
        {host.protocolSsh ? (
          <IconButton label={t.hosts.openTerminal} icon={SquareTerminal} size="sm" onClick={() => actions.connect(host)} />
        ) : null}
        {host.protocolSftp ? (
          <IconButton
            label={t.hosts.openFiles}
            icon={FolderOpen}
            size="sm"
            aria-disabled="true"
            title={t.hosts.filesLater}
            className={styles.later}
          />
        ) : null}
        <IconButton label={t.hosts.edit} icon={Pencil} size="sm" onClick={() => actions.edit(host)} />
      </div>
    </Card>
  );
}
```

**`ui/src/screens/hosts/HostCard.module.css`**
```css
.card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  padding: 16px;
}

.card:hover {
  border-color: var(--bd-hover);
}

.card[data-selected="true"] {
  border-color: var(--accent);
}

.select {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.avatar {
  display: grid;
  flex: 0 0 38px;
  place-items: center;
  height: 38px;
  border-radius: 10px;
  background: var(--raised);
  font-family: var(--font-mono);
  font-size: 13px;
  font-weight: 500;
}

.titles {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.name {
  overflow: hidden;
  font-size: 15px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.address {
  overflow: hidden;
  color: var(--muted);
  font-family: var(--font-mono);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chips {
  display: flex;
  flex: 1;
  flex-wrap: wrap;
  align-content: flex-start;
  gap: 6px;
}

.footer {
  display: flex;
  justify-content: flex-end;
  gap: 4px;
  padding-top: 10px;
  border-top: 1px solid var(--bd);
}

.later,
.later:hover {
  opacity: 0.45;
  cursor: not-allowed;
}
```

**`ui/src/screens/hosts/HostDetail.tsx`**
```tsx
import { FolderOpen, Pencil, SquareTerminal, Trash2 } from "lucide-react";
import { useId, useState } from "react";
import { Button, Chip, Dialog } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { formatEndpoint, hostColorVar, hostProfile, initials, methodLabel } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import styles from "./HostDetail.module.css";
import { useHostActions } from "./useHostActions";

export function HostDetail() {
  const host = useHostsStore((state) => state.hosts.find((h) => h.id === state.selectedId) ?? null);
  if (!host) {
    return (
      <aside className={styles.panel} aria-label={t.hosts.details}>
        <p className={styles.empty}>{t.hosts.noSelection}</p>
      </aside>
    );
  }
  return <HostDetailPanel key={host.id} host={host} />;
}

function HostDetailPanel({ host }: { host: Host }) {
  const hosts = useHostsStore((state) => state.hosts);
  const groups = useHostsStore((state) => state.groups);
  const profiles = useHostsStore((state) => state.profiles);
  const actions = useHostActions();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const forwardsId = useId();

  const group = groups.find((g) => g.id === host.groupId)?.name ?? t.hosts.noGroup;
  const subtitle = host.source === "sshConfig" ? `${group} · ${t.hosts.fromSshConfig}` : group;
  const auth = host.auth;
  const profileText =
    auth.kind === "profile"
      ? (hostProfile(host, profiles)?.name ?? t.hosts.none)
      : t.hosts.customAuth(methodLabel(auth.auth.method));
  const jump = host.jumpHostId ? (hosts.find((h) => h.id === host.jumpHostId)?.name ?? t.hosts.none) : t.hosts.none;
  const facts: Array<[string, string, boolean]> = [
    [t.hosts.address, formatEndpoint(host.user, host.address, 22), true],
    [t.hosts.port, String(host.port), true],
    [t.hosts.profile, profileText, false],
    [t.hosts.jumpHost, jump, false],
    [t.hosts.startPath, host.sftp.remoteStart ?? t.hosts.none, true],
    [t.hosts.fingerprint, t.hosts.none, true],
  ];

  const remove = async () => {
    setConfirmDelete(false);
    await actions.remove(host);
  };

  return (
    <aside className={styles.panel} aria-label={t.hosts.details}>
      <div className={styles.head}>
        <span className={styles.avatar} style={{ color: hostColorVar(host.color) }} aria-hidden="true">
          {initials(host.name)}
        </span>
        <div className={styles.titles}>
          <h2 className={styles.name}>{host.name}</h2>
          <p className={styles.subtitle}>{subtitle}</p>
        </div>
      </div>
      <div className={styles.actions}>
        <Button
          variant="primary"
          size="lg"
          icon={SquareTerminal}
          disabled={!host.protocolSsh}
          onClick={() => actions.connect(host)}
        >
          {t.hosts.terminal}
        </Button>
        <Button size="lg" icon={FolderOpen} aria-disabled="true" title={t.hosts.filesLater} className={styles.later}>
          {t.hosts.files}
        </Button>
      </div>
      <dl className={styles.facts}>
        {facts.map(([label, value, mono]) => (
          <div key={label} className={styles.fact}>
            <dt>{label}</dt>
            <dd className={mono ? "mono" : undefined}>{value}</dd>
          </div>
        ))}
      </dl>
      <section aria-labelledby={forwardsId}>
        <h3 id={forwardsId} className={styles.label}>
          {t.hosts.portForwarding}
        </h3>
        {host.forwards.length === 0 ? (
          <p className={styles.none}>{t.hosts.noRules}</p>
        ) : (
          <ul className={styles.rules}>
            {host.forwards.map((forward) => (
              <li key={forward.id} className={styles.rule}>
                <Chip tone="neutral" mono>
                  {t.hosts.forwardKind[forward.kind]}
                </Chip>
                <span className="mono">
                  {forward.kind === "dynamic" ? t.hosts.socks(forward.bind) : `${forward.bind} → ${forward.target}`}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
      <div className={styles.spacer} />
      <div className={styles.footer}>
        <Button icon={Pencil} onClick={() => actions.edit(host)}>
          {t.hosts.edit}
        </Button>
        {host.source === "linascp" ? (
          <Button variant="danger" icon={Trash2} onClick={() => setConfirmDelete(true)}>
            {t.hosts.delete}
          </Button>
        ) : null}
      </div>
      <Dialog
        open={confirmDelete}
        role="alertdialog"
        size="sm"
        title={t.hosts.deleteTitle}
        onClose={() => setConfirmDelete(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
              {t.common.cancel}
            </Button>
            <Button variant="danger" onClick={() => void remove()}>
              {t.common.delete}
            </Button>
          </>
        }
      >
        <p>{t.hosts.deleteText(host.name)}</p>
      </Dialog>
    </aside>
  );
}
```

**`ui/src/screens/hosts/HostDetail.module.css`**
```css
.panel {
  display: flex;
  flex: 1 1 300px;
  flex-direction: column;
  gap: 20px;
  min-width: 280px;
  max-width: 360px;
  padding: 24px;
  overflow-y: auto;
  border-left: 1px solid var(--bd);
  background: var(--s1);
}

.empty {
  margin: auto 0;
  color: var(--muted);
  text-align: center;
}

.head {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
}

.avatar {
  display: grid;
  flex: 0 0 52px;
  place-items: center;
  height: 52px;
  border-radius: 12px;
  background: var(--raised);
  font-family: var(--font-mono);
  font-size: 16px;
  font-weight: 500;
}

.titles {
  min-width: 0;
}

.name {
  overflow: hidden;
  font-size: 18px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.subtitle {
  color: var(--muted);
  font-size: 13px;
}

.actions {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.later,
.later:hover {
  opacity: 0.5;
  cursor: not-allowed;
}

.facts {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  font-size: 13px;
}

.fact {
  display: grid;
  grid-template-columns: 110px minmax(0, 1fr);
  gap: 12px;
}

.fact dt {
  color: var(--muted);
}

.fact dd {
  margin: 0;
  overflow-wrap: anywhere;
}

.label {
  margin-bottom: 8px;
  color: var(--muted);
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.rules {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding: 0;
  font-size: 13px;
  list-style: none;
}

.rule {
  display: flex;
  align-items: center;
  gap: 8px;
}

.none {
  color: var(--muted);
  font-size: 13px;
}

.spacer {
  flex: 1;
}

.footer {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
```

- [ ] **Step 7: Run the tests and the type check**

Run: `bun run test HostsScreen hostFormat quickConnect filterHosts AppShell`
Expected: PASS (HostsScreen 12, helpers 17, AppShell 6 — the shell still finds the "Server-Übersicht" heading).

Run: `bun run typecheck`
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add ui/src/model ui/src/test/fixtures.ts ui/src/screens/hosts ui/src/i18n/de.ts
git commit -m "feat(ui): add host overview with groups, profiles, filters, Quick Connect and details

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 20: Host / login-profile editor (EditorModal)

**Files:**
- Create: `ui/src/components/OptionCards/OptionCards.tsx` + `.module.css`, `ui/src/components/TextArea/TextArea.tsx` + `.module.css`
- Modify (full replacement): `ui/src/components/index.ts` (export `OptionCards`, `TextArea`)
- Create: `ui/src/stores/connectProgress.ts` (pure step-progress reducer shared with Task 21)
- Create: `ui/src/screens/connect/steps.ts`, `ui/src/screens/connect/PromptForm.tsx` + `.module.css` (shared with Task 21)
- Create: `ui/src/screens/editor/drafts.ts`, `validate.ts`, `form.ts`, `secrets.ts`, `useConnectionTest.ts`, `ConnectionTest.tsx`, `HostEditor.tsx`, `ProfileEditor.tsx`, `GeneralTab.tsx`, `AuthTab.tsx`, `AuthMethodFields.tsx`, `ConnectionTab.tsx`, `ForwardsEditor.tsx`, `SftpTab.tsx`, `ColorSwatches.tsx`, `Editor.module.css`
- Modify (full replacement of the stub): `ui/src/screens/editor/EditorModal.tsx`
- Modify (full replacement): `ui/src/shell/AppShell.test.tsx` (new editor titles), `ui/src/shell/ModalLayer.tsx` (key the editor by target)
- Modify: `ui/src/i18n/de.ts` (add `t.pipeline`, `t.editor`)
- Test: `ui/src/components/OptionCards/OptionCards.test.tsx`, `ui/src/components/TextArea/TextArea.test.tsx`, `ui/src/stores/connectProgress.test.ts`, `ui/src/screens/connect/steps.test.ts`, `ui/src/screens/connect/PromptForm.test.tsx`, `ui/src/screens/editor/drafts.test.ts`, `ui/src/screens/editor/validate.test.ts`, `ui/src/screens/editor/EditorModal.test.tsx`

**Interfaces:**
- Consumes: Task 19 `model/defaults` (`newHost`, `newProfile`, `nowSeconds`), `model/hostFormat` (`HOST_COLORS`, `HostColor`, `isHostColor`, `hostColorVar`, `initials`, `formatEndpoint`, `methodLabel`); `useHostsStore` (`hosts, groups, profiles, saveHost, saveProfile, saveGroup, select`); `api.secretSet`, `api.connectTest`, `api.connectAnswer`, `api.connectCancel`, `api.createEventChannel`, `api.newId`; `open()` from `@tauri-apps/plugin-dialog`.
- Produces:
  - Components: `OptionCards<T>{ label; value; options: { value; label; hint? }[]; onChange; className? }` (radiogroup of selectable cards), `TextArea{ label; value; onChange(value); error?; hint?; mono?; rows? … textarea attrs }`.
  - `stores/connectProgress.ts`: `interface StepProgress { steps: Step[]; current?: Step; done: Step[]; failedStep?: Step; log: string[] }`, `EMPTY_PROGRESS`, `MAX_LOG_LINES = 500`, `applyConnectEvent<T extends StepProgress>(state: T, event: ConnectEvent): T` (handles `steps/stepStarted/stepDone/stepFailed/log`, ignores the rest), `progressRatio(p)`, `asCoreError(error: unknown): CoreError`, `fireAndForget(task: () => Promise<unknown> | undefined): void`.
  - `screens/connect/steps.ts`: `stepLabel(step, jumpName?)`, `stepState(progress, step)`, `stepItems(progress, { jumpName?, meta?, detail? }): StepItem[]` (detail goes to the active step), `errorTitle(error: CoreError)`.
  - `screens/connect/PromptForm.tsx`: `type InlinePrompt` (password | passphrase | keyboardInteractive), `isInlinePrompt(prompt)`, `PromptForm{ prompt: InlinePrompt; onAnswer(answer: PromptAnswer); showSave?: boolean (default true) }`.
  - `screens/editor/drafts.ts`: `EditorTab`, `AuthSource`, `NO_GROUP`, `NEW_GROUP`, `NO_JUMP`, `AuthDraft`, `SftpDraft`, `HostDraft`, `ProfileDraft`, `hostToDraft`, `draftToHost(draft, groupId)`, `draftGroupId(draft)`, `profileToDraft`, `draftToProfile`, `draftToAuthConfig`, `parseTags`, `parseEnv`, `formatEnv`.
  - `screens/editor/validate.ts`: `HostField`, `ProfileField`, `FieldErrors<F>`, `HOST_FIELD_TAB`, `validateHost(draft)`, `validateProfile(draft)`, `validatePort(text)`, `envError(text)`, `isNonNegativeInt(text)`, `hasErrors(errors)`, `errorTabs(errors)`.
  - `EditorModal({ target })` (same props as the stub). Titles: "Neuer Host" / "Host bearbeiten" / "Neues Login-Profil" / "Login-Profil bearbeiten".

- [ ] **Step 1: Write the failing tests for the new components and pure modules**

**`ui/src/components/OptionCards/OptionCards.test.tsx`**
```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { OptionCards } from "./OptionCards";

const OPTIONS = [
  { value: "integrate", label: "Integrieren", hint: "Nur lesen" },
  { value: "mirror", label: "Spiegeln", hint: "Zusätzlich schreiben" },
] as const;

function Harness() {
  const [value, setValue] = useState<"integrate" | "mirror">("integrate");
  return <OptionCards label="ssh-Modus" value={value} options={OPTIONS} onChange={setValue} />;
}

describe("OptionCards", () => {
  it("is a labelled radiogroup with hints as descriptions", () => {
    render(<Harness />);
    expect(screen.getByRole("radiogroup", { name: "ssh-Modus" })).toBeInTheDocument();
    const integrate = screen.getByRole("radio", { name: "Integrieren" });
    expect(integrate).toHaveAttribute("aria-checked", "true");
    expect(integrate).toHaveAccessibleDescription("Nur lesen");
    expect(integrate).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "Spiegeln" })).toHaveAttribute("tabindex", "-1");
  });

  it("selects by click and arrow keys", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    expect(screen.getByRole("radio", { name: "Spiegeln" })).toHaveAttribute("aria-checked", "true");
    fireEvent.keyDown(screen.getByRole("radio", { name: "Spiegeln" }), { key: "ArrowRight" });
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveFocus();
  });
});
```

**`ui/src/components/TextArea/TextArea.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TextArea } from "./TextArea";

describe("TextArea", () => {
  it("is labelled and reports changes", async () => {
    const onChange = vi.fn();
    render(<TextArea label="Notizen" value="" onChange={onChange} hint="Optional" />);
    const area = screen.getByLabelText("Notizen");
    expect(area.tagName).toBe("TEXTAREA");
    expect(area).toHaveAccessibleDescription("Optional");
    await userEvent.type(area, "x");
    expect(onChange).toHaveBeenCalledWith("x");
  });

  it("shows an error", () => {
    render(<TextArea label="Umgebungsvariablen" value="kaputt" onChange={() => undefined} error="Zeile 1: Format KEY=VALUE" />);
    expect(screen.getByLabelText("Umgebungsvariablen")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Zeile 1: Format KEY=VALUE")).toBeInTheDocument();
  });
});
```

**`ui/src/stores/connectProgress.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import type { ConnectEvent } from "../ipc/api";
import { applyConnectEvent, asCoreError, EMPTY_PROGRESS, MAX_LOG_LINES, progressRatio, type StepProgress } from "./connectProgress";

const run = (events: ConnectEvent[]): StepProgress => events.reduce(applyConnectEvent, EMPTY_PROGRESS);

describe("applyConnectEvent", () => {
  it("tracks the planned, current, done and failed steps", () => {
    const progress = run([
      { kind: "steps", steps: ["tcp", "hostKey", "auth"] },
      { kind: "stepStarted", step: "tcp" },
      { kind: "stepDone", step: "tcp" },
      { kind: "stepStarted", step: "hostKey" },
    ]);
    expect(progress).toMatchObject({ steps: ["tcp", "hostKey", "auth"], done: ["tcp"], current: "hostKey" });
    expect(progressRatio(progress)).toBeCloseTo(1 / 3);
    const failed = applyConnectEvent(progress, {
      kind: "stepFailed",
      step: "hostKey",
      error: { type: "hostKeyRejected", message: "rejected" },
    });
    expect(failed).toMatchObject({ current: undefined, failedStep: "hostKey" });
  });

  it("keeps extra fields and ignores prompt/connected/failed events", () => {
    const state = { ...EMPTY_PROGRESS, attempt: 2 };
    const next = applyConnectEvent(state, { kind: "connected", sessionId: "s1" });
    expect(next).toBe(state);
    expect(applyConnectEvent(state, { kind: "log", line: "debug1: x" })).toMatchObject({ attempt: 2, log: ["debug1: x"] });
  });

  it("caps the log", () => {
    let state: StepProgress = EMPTY_PROGRESS;
    for (let i = 0; i < MAX_LOG_LINES + 5; i += 1) state = applyConnectEvent(state, { kind: "log", line: `l${i}` });
    expect(state.log).toHaveLength(MAX_LOG_LINES);
    expect(state.log[0]).toBe("l5");
  });

  it("starts over when a new step plan arrives", () => {
    const progress = run([
      { kind: "steps", steps: ["tcp"] },
      { kind: "stepDone", step: "tcp" },
      { kind: "steps", steps: ["tcp", "auth"] },
    ]);
    expect(progress).toMatchObject({ steps: ["tcp", "auth"], done: [] });
    expect(progressRatio(EMPTY_PROGRESS)).toBe(0);
  });
});

describe("asCoreError", () => {
  it("passes CoreErrors through and wraps anything else", () => {
    const error = { type: "network", kind: "dns", message: "nope" } as const;
    expect(asCoreError(error)).toBe(error);
    expect(asCoreError(new Error("boom"))).toEqual({ type: "internal", message: "boom" });
  });
});
```

**`ui/src/screens/connect/steps.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { EMPTY_PROGRESS } from "../../stores/connectProgress";
import { errorTitle, stepItems, stepLabel, stepState } from "./steps";

describe("steps", () => {
  const progress = { ...EMPTY_PROGRESS, steps: ["jumpHost", "tcp", "auth"] as const, done: ["jumpHost" as const], current: "tcp" as const };

  it("labels steps in German", () => {
    expect(stepLabel("jumpHost", "bastion")).toBe("Jump-Host bastion verbinden");
    expect(stepLabel("hostKey")).toBe("Host-Key prüfen");
    expect(stepLabel("shell")).toBe("Shell starten");
  });

  it("derives step states and items with meta and detail", () => {
    const p = { ...progress, steps: [...progress.steps] };
    expect(stepState(p, "jumpHost")).toBe("done");
    expect(stepState(p, "tcp")).toBe("active");
    expect(stepState(p, "auth")).toBe("pending");
    expect(stepState({ ...p, failedStep: "auth" }, "auth")).toBe("failed");
    const items = stepItems(p, { jumpName: "bastion", meta: { tcp: "10.0.1.12:22", auth: "Profil deploy" }, detail: "prompt" });
    expect(items.map((i) => [i.label, i.state, i.meta, i.detail])).toEqual([
      ["Jump-Host bastion verbinden", "done", undefined, undefined],
      ["Verbindung herstellen", "active", "10.0.1.12:22", "prompt"],
      ["Anmelden", "pending", undefined, undefined],
    ]);
  });

  it("titles errors by category", () => {
    expect(errorTitle({ type: "network", kind: "timeout", message: "" })).toBe("Zeitüberschreitung");
    expect(errorTitle({ type: "network", kind: "other", message: "" })).toBe("Verbindung fehlgeschlagen");
    expect(errorTitle({ type: "auth", rejectedMethods: [], message: "" })).toBe("Anmeldung fehlgeschlagen");
    expect(errorTitle({ type: "cancelled" })).toBe("Abgebrochen");
    expect(errorTitle({ type: "config", message: "host x not found" })).toBe("host x not found");
  });
});
```

**`ui/src/screens/connect/PromptForm.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PromptForm } from "./PromptForm";

describe("PromptForm", () => {
  it("submits a password and the save flag", async () => {
    const onAnswer = vi.fn();
    render(<PromptForm prompt={{ kind: "password", user: "deploy", host: "10.0.1.12", retry: false }} onAnswer={onAnswer} />);
    expect(screen.getByLabelText("Passwort für deploy@10.0.1.12")).toHaveAttribute("type", "password");
    expect(screen.getByRole("checkbox", { name: "Passwort im Schlüsselbund speichern" })).toBeChecked();
    await userEvent.type(screen.getByLabelText("Passwort für deploy@10.0.1.12"), "pw");
    await userEvent.click(screen.getByRole("button", { name: "Anmelden" }));
    expect(onAnswer).toHaveBeenCalledWith({ kind: "secret", value: "pw", save: true });
  });

  it("validates empty input and can be cancelled", async () => {
    const onAnswer = vi.fn();
    render(<PromptForm prompt={{ kind: "passphrase", keyPath: "~/.ssh/id_ed25519" }} onAnswer={onAnswer} />);
    expect(screen.getByRole("checkbox", { name: "Passphrase im Schlüsselbund speichern" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Anmelden" }));
    expect(screen.getByText("Bitte einen Wert eingeben.")).toBeInTheDocument();
    expect(onAnswer).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(onAnswer).toHaveBeenCalledWith({ kind: "cancel" });
  });

  it("shows the retry hint and can hide the save option", async () => {
    const onAnswer = vi.fn();
    render(
      <PromptForm prompt={{ kind: "password", user: "deploy", host: "h", retry: true }} onAnswer={onAnswer} showSave={false} />,
    );
    expect(screen.getByText("Passwort wurde abgelehnt. Bitte erneut eingeben.")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Passwort für deploy@h"), "x{Enter}");
    expect(onAnswer).toHaveBeenCalledWith({ kind: "secret", value: "x", save: false });
  });

  it("answers keyboard-interactive prompts respecting echo", async () => {
    const onAnswer = vi.fn();
    render(
      <PromptForm
        prompt={{
          kind: "keyboardInteractive",
          name: "2FA",
          instructions: "Bitte Code eingeben",
          prompts: [
            { text: "Password: ", echo: false },
            { text: "Verification code: ", echo: true },
          ],
        }}
        onAnswer={onAnswer}
      />,
    );
    expect(screen.getByText("2FA")).toBeInTheDocument();
    expect(screen.getByText("Bitte Code eingeben")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("Verification code")).toHaveAttribute("type", "text");
    await userEvent.type(screen.getByLabelText("Password"), "pw");
    await userEvent.type(screen.getByLabelText("Verification code"), "123456");
    await userEvent.click(screen.getByRole("button", { name: "Senden" }));
    expect(onAnswer).toHaveBeenCalledWith({ kind: "responses", values: ["pw", "123456"] });
  });
});
```

**`ui/src/screens/editor/drafts.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { makeHost, makeProfile } from "../../test/fixtures";
import {
  draftGroupId,
  draftToHost,
  draftToProfile,
  formatEnv,
  hostToDraft,
  NEW_GROUP,
  parseEnv,
  parseTags,
  profileToDraft,
} from "./drafts";

describe("env and tags", () => {
  it("parses KEY=VALUE lines and formats them back", () => {
    const env = parseEnv("LANG=de_DE.UTF-8\n\n  EDITOR=vim  \nEMPTY=\nX=a=b");
    expect(env).toEqual([
      { key: "LANG", value: "de_DE.UTF-8" },
      { key: "EDITOR", value: "vim" },
      { key: "EMPTY", value: "" },
      { key: "X", value: "a=b" },
    ]);
    expect(formatEnv(env)).toBe("LANG=de_DE.UTF-8\nEDITOR=vim\nEMPTY=\nX=a=b");
  });

  it("splits, trims and dedupes tags", () => {
    expect(parseTags(" nginx, docker,,nginx ")).toEqual(["nginx", "docker"]);
    expect(parseTags("")).toEqual([]);
  });
});

describe("host drafts", () => {
  const host = makeHost({
    port: 2222,
    groupId: "g1",
    tags: ["a", "b"],
    env: [{ key: "LANG", value: "C" }],
    jumpHostId: "h-jump",
    startCommand: "tmux",
    sftp: { remoteStart: "/srv", localStart: null, transferMode: "auto", parallel: 3, uploadPerms: "0644", bandwidthLimitKbps: 500, sftpServerCmd: null },
  });

  it("round-trips every field", () => {
    const draft = hostToDraft(host);
    expect(draft).toMatchObject({ port: "2222", groupId: "g1", tags: "a, b", env: "LANG=C", authSource: "custom", user: "deploy" });
    const back = draftToHost(draft, draftGroupId(draft));
    expect({ ...back, updatedAt: host.updatedAt }).toEqual(host);
  });

  it("maps auth sources and the secret flag", () => {
    const draft = hostToDraft(host);
    const withPassword = draftToHost({ ...draft, auth: { ...draft.auth, password: "pw" } }, null);
    expect(withPassword.auth).toEqual({ kind: "custom", auth: { method: "password", keyPath: null, hasSecret: true } });
    const viaProfile = draftToHost({ ...draft, authSource: "profile", profileId: "p1" }, null);
    expect(viaProfile.auth).toEqual({ kind: "profile", profileId: "p1" });
    expect(viaProfile.user).toBeNull();
    const key = draftToHost({ ...draft, auth: { ...draft.auth, method: "key", keyPath: " ~/.ssh/k ", passphrase: "" } }, null);
    expect(key.auth).toEqual({ kind: "custom", auth: { method: "key", keyPath: "~/.ssh/k", hasSecret: false } });
  });

  it("turns ssh-config hosts into LinaSCP hosts and resolves the group id", () => {
    const draft = hostToDraft(makeHost({ source: "sshConfig", color: null }));
    expect(draft.color).toBe("accent");
    expect(draftToHost(draft, null).source).toBe("linascp");
    expect(draftGroupId({ ...draft, groupId: NEW_GROUP })).toBeNull();
    expect(draftGroupId({ ...draft, groupId: "" })).toBeNull();
  });
});

describe("profile drafts", () => {
  it("round-trips and trims", () => {
    const profile = makeProfile();
    const draft = profileToDraft(profile);
    expect({ ...draftToProfile(draft), updatedAt: 1 }).toEqual(profile);
    expect(draftToProfile({ ...draft, name: " ops ", auth: { ...draft.auth, passphrase: "x" } })).toMatchObject({
      name: "ops",
      auth: { hasSecret: true },
    });
  });
});
```

**`ui/src/screens/editor/validate.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { makeHost, makeProfile } from "../../test/fixtures";
import { hostToDraft, NEW_GROUP, profileToDraft } from "./drafts";
import { envError, errorTabs, hasErrors, isNonNegativeInt, validateHost, validatePort, validateProfile } from "./validate";

const valid = hostToDraft(makeHost());

describe("field rules", () => {
  it("accepts ports 1–65535 only", () => {
    expect(validatePort("22")).toBeUndefined();
    expect(validatePort("65535")).toBeUndefined();
    for (const port of ["0", "65536", "", "2a", "-1", "1.5"]) expect(validatePort(port), port).toBe("Port muss zwischen 1 und 65535 liegen");
  });

  it("accepts non-negative integers only", () => {
    expect(isNonNegativeInt("0")).toBe(true);
    expect(isNonNegativeInt(" 30 ")).toBe(true);
    expect(isNonNegativeInt("-1")).toBe(false);
    expect(isNonNegativeInt("1.5")).toBe(false);
    expect(isNonNegativeInt("")).toBe(false);
  });

  it("requires KEY=VALUE env lines", () => {
    expect(envError("LANG=C\n\nA_1=")).toBeUndefined();
    expect(envError("LANG=C\nkaputt")).toBe("Zeile 2: Format KEY=VALUE");
    expect(envError("1X=a")).toBe("Zeile 1: Format KEY=VALUE");
  });
});

describe("validateHost", () => {
  it("accepts a valid draft", () => {
    expect(validateHost(valid)).toEqual({});
    expect(hasErrors(validateHost(valid))).toBe(false);
  });

  it("reports every invalid field", () => {
    const errors = validateHost({
      ...valid,
      name: " ",
      address: "",
      port: "0",
      groupId: NEW_GROUP,
      newGroupName: "",
      keepalive: "x",
      timeout: "-5",
      env: "nope",
      forwards: [{ id: "f", kind: "local", bind: "8080", target: "", autostart: false }],
      sftp: { ...valid.sftp, parallel: "", bandwidthLimit: "fast" },
    });
    expect(errors).toEqual({
      name: "Pflichtfeld",
      address: "Pflichtfeld",
      port: "Port muss zwischen 1 und 65535 liegen",
      newGroupName: "Gruppennamen angeben",
      keepalive: "Ganze Zahl ≥ 0 eingeben",
      timeout: "Ganze Zahl ≥ 0 eingeben",
      env: "Zeile 1: Format KEY=VALUE",
      forwards: "Jede Regel braucht Bind-Adresse und Ziel",
      parallel: "Ganze Zahl ≥ 0 eingeben",
      bandwidthLimit: "Ganze Zahl ≥ 0 eingeben",
    });
    expect(errorTabs(errors)).toEqual(["general", "connection", "sftp"]);
  });

  it("checks auth: profile selection, key path, address without spaces", () => {
    expect(validateHost({ ...valid, authSource: "profile", profileId: "" })).toEqual({ profileId: "Login-Profil auswählen" });
    expect(validateHost({ ...valid, auth: { ...valid.auth, method: "key", keyPath: "" } })).toEqual({
      keyPath: "Pfad zum privaten Schlüssel angeben",
    });
    expect(validateHost({ ...valid, authSource: "profile", profileId: "p", auth: { ...valid.auth, method: "key", keyPath: "" } })).toEqual({});
    expect(validateHost({ ...valid, address: "a b" })).toEqual({ address: "Die Adresse darf keine Leerzeichen enthalten" });
    expect(validateHost({ ...valid, forwards: [{ id: "f", kind: "dynamic", bind: "1080", target: "", autostart: false }] })).toEqual({});
  });
});

describe("validateProfile", () => {
  it("requires name, user and a key path for key auth", () => {
    const draft = profileToDraft(makeProfile());
    expect(validateProfile(draft)).toEqual({});
    expect(validateProfile({ ...draft, name: "", user: "", auth: { ...draft.auth, keyPath: "" } })).toEqual({
      name: "Pflichtfeld",
      user: "Pflichtfeld",
      keyPath: "Pfad zum privaten Schlüssel angeben",
    });
  });
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `bun run test OptionCards TextArea connectProgress steps PromptForm drafts validate`
Expected: FAIL — unresolved imports (`./OptionCards`, `./TextArea`, `./connectProgress`, `./steps`, `./PromptForm`, `./drafts`, `./validate`).

- [ ] **Step 3: Implement the components, the shared connect helpers, drafts and validation**

**`ui/src/components/OptionCards/OptionCards.tsx`**
```tsx
import { type KeyboardEvent, useId, useRef } from "react";
import { cx } from "../cx";
import { nextIndex } from "../roving";
import styles from "./OptionCards.module.css";

export interface OptionCardItem<T extends string> {
  value: T;
  label: string;
  hint?: string;
}

export interface OptionCardsProps<T extends string> {
  /** Accessible name of the radiogroup. */
  label: string;
  value: T;
  options: readonly OptionCardItem<T>[];
  onChange: (value: T) => void;
  className?: string;
}

/** Radiogroup of bordered cards (label + hint), e.g. "Login-Profil verwenden" / "Individuell". */
export function OptionCards<T extends string>({ label, value, options, onChange, className }: OptionCardsProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const baseId = useId();
  const selectedIndex = options.findIndex((option) => option.value === value);
  const tabStop = selectedIndex >= 0 ? selectedIndex : 0;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, options.length, "both");
    const option = next === null ? undefined : options[next];
    if (next === null || !option) return;
    event.preventDefault();
    onChange(option.value);
    refs.current[next]?.focus();
  };

  return (
    <div role="radiogroup" aria-label={label} className={cx(styles.group, className)}>
      {options.map((option, index) => {
        const checked = option.value === value;
        const labelId = `${baseId}-${index}-label`;
        const hintId = `${baseId}-${index}-hint`;
        return (
          <button
            key={option.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-labelledby={labelId}
            aria-describedby={option.hint ? hintId : undefined}
            tabIndex={index === tabStop ? 0 : -1}
            className={cx(styles.card, checked && styles.checked)}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            <span id={labelId} className={styles.label}>
              {option.label}
            </span>
            {option.hint ? (
              <span id={hintId} className={styles.hint}>
                {option.hint}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
```

**`ui/src/components/OptionCards/OptionCards.module.css`**
```css
.group {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 10px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 14px 16px;
  border: 1px solid var(--bd-strong);
  border-radius: 10px;
  background: var(--s1);
  color: var(--text);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.15s;
}

.card:hover {
  border-color: var(--bd-hover);
}

.checked,
.checked:hover {
  border-color: var(--accent);
  box-shadow: inset 0 0 0 1px var(--accent);
}

.label {
  font-weight: 500;
}

.hint {
  color: var(--muted);
  font-size: 12px;
}
```

**`ui/src/components/TextArea/TextArea.tsx`**
```tsx
import { type Ref, type TextareaHTMLAttributes, useId } from "react";
import { cx } from "../cx";
import { fieldDescription } from "../field";
import field from "../field.module.css";
import styles from "./TextArea.module.css";

export interface TextAreaProps extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "onChange"> {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string | null;
  hint?: string;
  mono?: boolean;
  ref?: Ref<HTMLTextAreaElement>;
}

export function TextArea({ label, value, onChange, error, hint, mono = false, id, className, rows = 3, ...rest }: TextAreaProps) {
  const generatedId = useId();
  const areaId = id ?? generatedId;
  const { hintId, errorId, describedBy } = fieldDescription(areaId, hint, error);
  return (
    <div className={cx(field.field, className)}>
      <label className={field.label} htmlFor={areaId}>
        {label}
      </label>
      <textarea
        {...rest}
        id={areaId}
        rows={rows}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy}
        className={cx(field.control, styles.area, mono && field.mono, error && field.invalid)}
      />
      {hint ? (
        <p id={hintId} className={field.hint}>
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className={field.error}>
          {error}
        </p>
      ) : null}
    </div>
  );
}
```

**`ui/src/components/TextArea/TextArea.module.css`**
```css
.area {
  height: auto;
  min-height: 72px;
  padding: 10px 12px;
  line-height: 1.45;
  resize: vertical;
}
```

**`ui/src/components/index.ts`**
```ts
export { Button, type ButtonProps, type ButtonSize, type ButtonVariant } from "./Button/Button";
export { Card, type CardProps } from "./Card/Card";
export { Checkbox, type CheckboxProps } from "./Checkbox/Checkbox";
export { Chip, type ChipProps } from "./Chip/Chip";
export { cx } from "./cx";
export { Dialog, type DialogProps } from "./Dialog/Dialog";
export { IconButton, type IconButtonProps } from "./IconButton/IconButton";
export { type OptionCardItem, OptionCards, type OptionCardsProps } from "./OptionCards/OptionCards";
export { ProgressBar, type ProgressBarProps } from "./ProgressBar/ProgressBar";
export { nextIndex, type Orientation } from "./roving";
export { Segmented, type SegmentedOption, type SegmentedProps } from "./Segmented/Segmented";
export { Select, type SelectOption, type SelectProps } from "./Select/Select";
export { StepList, type StepItem, type StepListProps, type StepState } from "./StepList/StepList";
export { type TabItem, Tabs, type TabsProps, tabId, tabPanelId } from "./Tabs/Tabs";
export { TextArea, type TextAreaProps } from "./TextArea/TextArea";
export { TextField, type TextFieldProps } from "./TextField/TextField";
export { type ToastApi, type ToastOptions, ToastProvider, useToast } from "./Toast/Toast";
export type { Tone } from "./tone";
```

**`ui/src/stores/connectProgress.ts`**
```ts
import type { ConnectEvent, CoreError, Step } from "../ipc/api";
import { errorMessage, isCoreError } from "../ipc/errors";

/** Progress of one pipeline run, derived from `ConnectEvent`s (shared by the connect flow and "Verbindung testen"). */
export interface StepProgress {
  steps: Step[];
  current?: Step;
  done: Step[];
  failedStep?: Step;
  log: string[];
}

export const EMPTY_PROGRESS: StepProgress = { steps: [], done: [], log: [] };
export const MAX_LOG_LINES = 500;

/** Reduces progress events; `prompt`, `connected` and `failed` are handled by the caller and return `state` unchanged. */
export function applyConnectEvent<T extends StepProgress>(state: T, event: ConnectEvent): T {
  switch (event.kind) {
    case "steps":
      return { ...state, steps: event.steps, done: [], current: undefined, failedStep: undefined };
    case "stepStarted":
      return { ...state, current: event.step };
    case "stepDone":
      return {
        ...state,
        done: state.done.includes(event.step) ? state.done : [...state.done, event.step],
        current: state.current === event.step ? undefined : state.current,
      };
    case "stepFailed":
      return { ...state, current: undefined, failedStep: event.step };
    case "log": {
      const log = [...state.log, event.line];
      return { ...state, log: log.length > MAX_LOG_LINES ? log.slice(log.length - MAX_LOG_LINES) : log };
    }
    default:
      return state;
  }
}

export function progressRatio(progress: StepProgress): number {
  return progress.steps.length === 0 ? 0 : progress.done.length / progress.steps.length;
}

/** Command rejections are `CoreError`s; anything else (bugs, mocks) becomes `internal`. */
export function asCoreError(error: unknown): CoreError {
  return isCoreError(error) ? error : { type: "internal", message: errorMessage(error) };
}

/** Runs a best-effort IPC call whose outcome does not matter (cancel, answer of a stale prompt). */
export function fireAndForget(task: () => Promise<unknown> | undefined): void {
  void (async () => {
    try {
      await task();
    } catch {
      // ignored on purpose
    }
  })();
}
```

**`ui/src/screens/connect/steps.ts`**
```ts
import type { ReactNode } from "react";
import type { StepItem, StepState } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Step } from "../../ipc/api";
import type { StepProgress } from "../../stores/connectProgress";

export function stepLabel(step: Step, jumpName = ""): string {
  switch (step) {
    case "jumpHost":
      return t.pipeline.steps.jumpHost(jumpName);
    case "tcp":
      return t.pipeline.steps.tcp;
    case "hostKey":
      return t.pipeline.steps.hostKey;
    case "auth":
      return t.pipeline.steps.auth;
    case "channel":
      return t.pipeline.steps.channel;
    case "shell":
      return t.pipeline.steps.shell;
  }
}

export function stepState(progress: StepProgress, step: Step): StepState {
  if (progress.failedStep === step) return "failed";
  if (progress.done.includes(step)) return "done";
  if (progress.current === step) return "active";
  return "pending";
}

export interface StepItemOptions {
  jumpName?: string;
  /** Mono text shown once a step is reached. */
  meta?: Partial<Record<Step, string>>;
  /** Rendered below the active step (inline prompts). */
  detail?: ReactNode;
}

export function stepItems(progress: StepProgress, options: StepItemOptions = {}): StepItem[] {
  return progress.steps.map((step) => {
    const state = stepState(progress, step);
    return {
      id: step,
      label: stepLabel(step, options.jumpName),
      state,
      meta: state === "pending" ? undefined : options.meta?.[step],
      detail: state === "active" ? options.detail : undefined,
    };
  });
}

/** Short German title of a pipeline error (backend messages of other categories are shown as-is). */
export function errorTitle(error: CoreError): string {
  switch (error.type) {
    case "network":
      return error.kind === "other" ? t.pipeline.errors.network : t.pipeline.errors[error.kind];
    case "auth":
      return t.pipeline.errors.auth;
    case "hostKeyRejected":
      return t.pipeline.errors.hostKeyRejected;
    case "cancelled":
      return t.pipeline.errors.cancelled;
    default:
      return error.message;
  }
}
```

**`ui/src/screens/connect/PromptForm.tsx`**
```tsx
import { type FormEvent, useState } from "react";
import { Button, Checkbox, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { Prompt, PromptAnswer } from "../../ipc/api";
import styles from "./PromptForm.module.css";

export type InlinePrompt = Extract<Prompt, { kind: "password" | "passphrase" | "keyboardInteractive" }>;

export function isInlinePrompt(prompt: Prompt): prompt is InlinePrompt {
  return prompt.kind === "password" || prompt.kind === "passphrase" || prompt.kind === "keyboardInteractive";
}

export interface PromptFormProps {
  prompt: InlinePrompt;
  onAnswer: (answer: PromptAnswer) => void;
  /** Offer "im Schlüsselbund speichern" (password/passphrase). */
  showSave?: boolean;
}

/** Inline answer form for password, passphrase and keyboard-interactive (2FA) prompts. */
export function PromptForm({ prompt, onAnswer, showSave = true }: PromptFormProps) {
  return prompt.kind === "keyboardInteractive" ? (
    <KeyboardInteractiveForm prompt={prompt} onAnswer={onAnswer} />
  ) : (
    <SecretForm prompt={prompt} onAnswer={onAnswer} showSave={showSave} />
  );
}

function SecretForm({
  prompt,
  onAnswer,
  showSave,
}: {
  prompt: Extract<Prompt, { kind: "password" | "passphrase" }>;
  onAnswer: (answer: PromptAnswer) => void;
  showSave: boolean;
}) {
  const [value, setValue] = useState("");
  const [save, setSave] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const isPassword = prompt.kind === "password";
  const label = isPassword ? t.pipeline.prompt.password(`${prompt.user}@${prompt.host}`) : t.pipeline.prompt.passphrase(prompt.keyPath);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!value) {
      setError(t.pipeline.prompt.empty);
      return;
    }
    onAnswer({ kind: "secret", value, save: showSave && save });
  };

  return (
    <form className={styles.form} onSubmit={submit}>
      {isPassword && prompt.retry ? <p className={styles.retry}>{t.pipeline.prompt.passwordRetry}</p> : null}
      <TextField
        type="password"
        label={label}
        value={value}
        autoFocus
        autoComplete="off"
        error={error}
        onChange={(next) => {
          setValue(next);
          setError(null);
        }}
      />
      {showSave ? (
        <Checkbox
          label={isPassword ? t.pipeline.prompt.savePassword : t.pipeline.prompt.savePassphrase}
          checked={save}
          onChange={setSave}
        />
      ) : null}
      <div className={styles.actions}>
        <Button size="sm" variant="ghost" onClick={() => onAnswer({ kind: "cancel" })}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit">
          {t.pipeline.prompt.submit}
        </Button>
      </div>
    </form>
  );
}

function KeyboardInteractiveForm({
  prompt,
  onAnswer,
}: {
  prompt: Extract<Prompt, { kind: "keyboardInteractive" }>;
  onAnswer: (answer: PromptAnswer) => void;
}) {
  const [values, setValues] = useState<string[]>(() => prompt.prompts.map(() => ""));

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onAnswer({ kind: "responses", values });
  };

  return (
    <form className={styles.form} onSubmit={submit}>
      {prompt.name ? <p className={styles.name}>{prompt.name}</p> : null}
      {prompt.instructions ? <p className={styles.instructions}>{prompt.instructions}</p> : null}
      {prompt.prompts.map((item, index) => (
        <TextField
          // Prompts have no identity besides their position.
          key={index}
          type={item.echo ? "text" : "password"}
          label={item.text.trim().replace(/:$/, "") || t.pipeline.prompt.response(index + 1)}
          value={values[index] ?? ""}
          autoFocus={index === 0}
          autoComplete="off"
          onChange={(next) => setValues((current) => current.map((v, i) => (i === index ? next : v)))}
        />
      ))}
      <div className={styles.actions}>
        <Button size="sm" variant="ghost" onClick={() => onAnswer({ kind: "cancel" })}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit">
          {t.pipeline.prompt.send}
        </Button>
      </div>
    </form>
  );
}
```

**`ui/src/screens/connect/PromptForm.module.css`**
```css
.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  border: 1px solid var(--bd);
  border-radius: 10px;
  background: var(--s1);
}

.name {
  font-weight: 600;
}

.instructions,
.retry {
  color: var(--text2);
  font-size: 13px;
}

.retry {
  color: var(--red);
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
```

**`ui/src/screens/editor/drafts.ts`**
```ts
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
```

**`ui/src/screens/editor/validate.ts`**
```ts
import { t } from "../../i18n/de";
import { type AuthDraft, type EditorTab, type HostDraft, NEW_GROUP, type ProfileDraft } from "./drafts";

export type HostField =
  | "name"
  | "address"
  | "port"
  | "newGroupName"
  | "profileId"
  | "keyPath"
  | "keepalive"
  | "timeout"
  | "env"
  | "forwards"
  | "parallel"
  | "bandwidthLimit";
export type ProfileField = "name" | "user" | "keyPath";
export type FieldErrors<F extends string> = Partial<Record<F, string>>;

export const HOST_FIELD_TAB: Record<HostField, EditorTab> = {
  name: "general",
  address: "general",
  port: "general",
  newGroupName: "general",
  profileId: "auth",
  keyPath: "auth",
  keepalive: "connection",
  timeout: "connection",
  env: "connection",
  forwards: "connection",
  parallel: "sftp",
  bandwidthLimit: "sftp",
};

const TAB_ORDER: readonly EditorTab[] = ["general", "auth", "connection", "sftp"];
const INT_RE = /^\d+$/;
const ENV_LINE_RE = /^[A-Za-z_][A-Za-z0-9_]*=/;

export function isNonNegativeInt(text: string): boolean {
  return INT_RE.test(text.trim());
}

export function validatePort(text: string): string | undefined {
  const value = text.trim();
  if (!INT_RE.test(value)) return t.editor.errors.port;
  const port = Number(value);
  return port >= 1 && port <= 65535 ? undefined : t.editor.errors.port;
}

/** First invalid non-empty line (1-based) of the env text area. */
export function envError(text: string): string | undefined {
  for (const [index, raw] of text.split("\n").entries()) {
    const line = raw.trim();
    if (line && !ENV_LINE_RE.test(line)) return t.editor.errors.env(index + 1);
  }
  return undefined;
}

function keyPathError(auth: AuthDraft): string | undefined {
  return auth.method === "key" && !auth.keyPath.trim() ? t.editor.errors.keyPath : undefined;
}

export function validateHost(draft: HostDraft): FieldErrors<HostField> {
  const errors: FieldErrors<HostField> = {};
  const nonNegative = t.editor.errors.nonNegative;
  if (!draft.name.trim()) errors.name = t.editor.errors.required;
  const address = draft.address.trim();
  if (!address) errors.address = t.editor.errors.required;
  else if (/\s/.test(address)) errors.address = t.editor.errors.address;
  const port = validatePort(draft.port);
  if (port) errors.port = port;
  if (draft.groupId === NEW_GROUP && !draft.newGroupName.trim()) errors.newGroupName = t.editor.errors.groupName;
  if (draft.authSource === "profile" && !draft.profileId) errors.profileId = t.editor.errors.profile;
  if (draft.authSource === "custom") {
    const keyPath = keyPathError(draft.auth);
    if (keyPath) errors.keyPath = keyPath;
  }
  if (!isNonNegativeInt(draft.keepalive)) errors.keepalive = nonNegative;
  if (!isNonNegativeInt(draft.timeout)) errors.timeout = nonNegative;
  const env = envError(draft.env);
  if (env) errors.env = env;
  if (draft.forwards.some((f) => !f.bind.trim() || (f.kind !== "dynamic" && !f.target.trim()))) {
    errors.forwards = t.editor.errors.forward;
  }
  if (!isNonNegativeInt(draft.sftp.parallel)) errors.parallel = nonNegative;
  if (draft.sftp.bandwidthLimit.trim() && !isNonNegativeInt(draft.sftp.bandwidthLimit)) errors.bandwidthLimit = nonNegative;
  return errors;
}

export function validateProfile(draft: ProfileDraft): FieldErrors<ProfileField> {
  const errors: FieldErrors<ProfileField> = {};
  if (!draft.name.trim()) errors.name = t.editor.errors.required;
  if (!draft.user.trim()) errors.user = t.editor.errors.required;
  const keyPath = keyPathError(draft.auth);
  if (keyPath) errors.keyPath = keyPath;
  return errors;
}

export function hasErrors(errors: object): boolean {
  return Object.keys(errors).length > 0;
}

/** Tabs that contain at least one error, in tab order. */
export function errorTabs(errors: FieldErrors<HostField>): EditorTab[] {
  const tabs = new Set((Object.keys(errors) as HostField[]).map((field) => HOST_FIELD_TAB[field]));
  return TAB_ORDER.filter((tab) => tabs.has(tab));
}
```

**`ui/src/i18n/de.ts`** (insert before `} as const;`)
```ts
  pipeline: {
    steps: {
      jumpHost: (name: string) => `Jump-Host ${name} verbinden`,
      tcp: "Verbindung herstellen",
      hostKey: "Host-Key prüfen",
      auth: "Anmelden",
      channel: "Kanal öffnen",
      shell: "Shell starten",
    },
    errors: {
      timeout: "Zeitüberschreitung",
      refused: "Verbindung abgelehnt",
      dns: "Host nicht gefunden",
      network: "Verbindung fehlgeschlagen",
      auth: "Anmeldung fehlgeschlagen",
      hostKeyRejected: "Host-Key abgelehnt",
      cancelled: "Abgebrochen",
    },
    prompt: {
      password: (target: string) => `Passwort für ${target}`,
      passwordRetry: "Passwort wurde abgelehnt. Bitte erneut eingeben.",
      passphrase: (keyPath: string) => `Passphrase für ${keyPath}`,
      savePassword: "Passwort im Schlüsselbund speichern",
      savePassphrase: "Passphrase im Schlüsselbund speichern",
      response: (n: number) => `Antwort ${n}`,
      submit: "Anmelden",
      send: "Senden",
      empty: "Bitte einen Wert eingeben.",
    },
  },
  editor: {
    newHost: "Neuer Host",
    editHost: "Host bearbeiten",
    newProfile: "Neues Login-Profil",
    editProfile: "Login-Profil bearbeiten",
    hostSubtitle: (endpoint: string, group: string) => `${endpoint} · ${group}`,
    profileSubtitle: (user: string, usedBy: number) =>
      `${user} · verwendet von ${usedBy === 1 ? "1 Host" : `${usedBy} Hosts`}`,
    sections: "Bereiche",
    tabs: { general: "Allgemein", auth: "Authentifizierung", connection: "Verbindung", sftp: "SFTP" },
    sshConfigHint:
      "Dieser Host stammt aus ~/.ssh/config. Beim Speichern entsteht ein LinaSCP-Host gleichen Namens, der den Eintrag überlagert. Die Datei selbst bleibt unverändert.",
    name: "Anzeigename",
    address: "Adresse (Host oder IP)",
    port: "Port",
    group: "Gruppe",
    noGroup: "Keine Gruppe",
    newGroup: "+ Neue Gruppe …",
    newGroupName: "Name der neuen Gruppe",
    tags: "Tags",
    tagsHint: "Mehrere Tags mit Komma trennen",
    protocols: "Protokolle",
    protocolSsh: "SSH-Terminal",
    protocolSftp: "SFTP",
    color: "Farbe",
    colors: { accent: "Türkis", blue: "Blau", violet: "Violett", amber: "Orange", pink: "Rosa", muted: "Grau" },
    notes: "Notizen",
    credentials: "Anmeldedaten",
    useProfile: "Login-Profil verwenden",
    useProfileHint: "Gemeinsame Zugangsdaten für mehrere Hosts",
    custom: "Individuell",
    customHint: "Nur für diesen Host",
    profile: "Login-Profil",
    chooseProfile: "Login-Profil wählen …",
    noProfiles: "Noch keine Login-Profile vorhanden – lege eines an.",
    usedBy: "Verwendet von",
    hostCount: (n: number) => (n === 1 ? "1 Host" : `${n} Hosts`),
    editProfileButton: "Profil bearbeiten",
    newProfileButton: "Neues Login-Profil",
    user: "Benutzer",
    method: "Methode",
    key: "Schlüssel",
    password: "Passwort",
    secretStored: "Gespeichert – leer lassen, um es zu behalten",
    secretHint: "Wird im Schlüsselbund des Systems gespeichert.",
    keyPath: "Privater Schlüssel",
    keyPathPlaceholder: "~/.ssh/id_ed25519",
    pickKey: "Datei wählen …",
    pickKeyTitle: "Privaten Schlüssel wählen",
    passphrase: "Passphrase",
    passphrasePlaceholder: "Optional",
    agentInfo: "Schlüssel werden vom laufenden SSH-Agent bezogen (SSH_AUTH_SOCK).",
    kbdInfo: "Der Server fragt beim Verbinden nach Eingaben, etwa Passwort und Einmalcode (2FA).",
    jumpHost: "Jump-Host (ProxyJump)",
    noJump: "Keiner",
    keepalive: "Keepalive-Intervall (s)",
    timeout: "Verbindungs-Timeout (s)",
    startCommand: "Startbefehl",
    startCommandPlaceholder: "z. B. tmux attach || tmux",
    env: "Umgebungsvariablen",
    envHint: "Eine Variable pro Zeile, z. B. LANG=de_DE.UTF-8",
    portForwarding: "Port-Forwarding",
    forwardsHint: "Regeln werden gespeichert und wirken ab Teilprojekt 5.",
    forwardKind: "Typ",
    forwardKinds: { local: "Lokal", remote: "Remote", dynamic: "Dynamisch (SOCKS)" },
    forwardBind: "Bind-Adresse",
    forwardBindPlaceholder: "127.0.0.1:8080",
    forwardTarget: "Ziel",
    forwardTargetPlaceholder: "localhost:80",
    autostart: "Auto-Start",
    removeForward: "Regel entfernen",
    addForward: "+ Regel hinzufügen",
    sftpHint: "Die SFTP-Einstellungen werden gespeichert und wirken ab Teilprojekt 3.",
    remoteStart: "Remote-Startverzeichnis",
    localStart: "Lokales Startverzeichnis",
    transferMode: "Übertragungsmodus",
    transferModes: { auto: "Automatisch", binary: "Binär", text: "Text" },
    parallel: "Parallele Übertragungen",
    uploadPerms: "Rechte beim Upload",
    uploadPermsPlaceholder: "0644",
    bandwidth: "Bandbreitenlimit (KB/s)",
    bandwidthPlaceholder: "Unbegrenzt",
    sftpServer: "SFTP-Server (z. B. mit sudo)",
    profileInfo: (usedBy: number) =>
      usedBy === 0
        ? "Login-Profile bündeln Benutzer und Anmeldedaten. Noch nutzt kein Host dieses Profil."
        : usedBy === 1
          ? "Login-Profile bündeln Benutzer und Anmeldedaten. Änderungen gelten für den Host, der dieses Profil nutzt."
          : `Login-Profile bündeln Benutzer und Anmeldedaten. Änderungen gelten für alle ${usedBy} Hosts, die dieses Profil nutzen.`,
    profileName: "Profilname",
    backToHost: "Zurück zum Host",
    test: "Verbindung testen",
    testCancel: "Test abbrechen",
    testRunning: "Teste Verbindung …",
    testOk: "● Verbindung erfolgreich, Anmeldung möglich",
    testFailed: (reason: string) => `● ${reason}`,
    testSteps: "Verbindungstest",
    hostKeyUnknown: (fingerprint: string) => `Unbekannter Host-Key: ${fingerprint}`,
    hostKeyChanged: "Der Host-Key hat sich geändert – der Test wird abgebrochen.",
    acceptOnce: "Nur dieses Mal",
    trust: "Vertrauen",
    invalidHint: (tabs: string) => `Bitte Eingaben prüfen: ${tabs}`,
    secretFailed: "Speichern im Schlüsselbund derzeit nicht möglich – das Geheimnis wird beim Verbinden erneut abgefragt.",
    errors: {
      required: "Pflichtfeld",
      address: "Die Adresse darf keine Leerzeichen enthalten",
      port: "Port muss zwischen 1 und 65535 liegen",
      nonNegative: "Ganze Zahl ≥ 0 eingeben",
      env: (line: number) => `Zeile ${line}: Format KEY=VALUE`,
      forward: "Jede Regel braucht Bind-Adresse und Ziel",
      keyPath: "Pfad zum privaten Schlüssel angeben",
      profile: "Login-Profil auswählen",
      groupName: "Gruppennamen angeben",
    },
  },
```

Run: `bun run test OptionCards TextArea connectProgress steps PromptForm drafts validate`
Expected: PASS (OptionCards 2, TextArea 2, connectProgress 5, steps 3, PromptForm 4, drafts 6, validate 7).

- [ ] **Step 4: Write the failing EditorModal tests**

**`ui/src/screens/editor/EditorModal.test.tsx`**
```tsx
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { type EditorTarget, useUiStore } from "../../stores/ui";
import { makeHost, makeProfile, withUsage } from "../../test/fixtures";
import { EditorModal } from "./EditorModal";

vi.mock("../../ipc/api");
vi.mock("@tauri-apps/plugin-dialog");

const deploy = makeProfile({ id: "p-deploy", name: "deploy", user: "deploy" });
const web = makeHost({ id: "h-web", name: "web-prod-01", user: "deploy" });
const bastion = makeHost({ id: "h-bastion", name: "bastion", address: "bastion.example.net" });
const config = makeHost({
  id: "h-cfg",
  name: "nas",
  address: "192.168.1.10",
  source: "sshConfig",
  color: null,
  auth: { kind: "custom", auth: { method: "agent", keyPath: null, hasSecret: false } },
});
const groups = [{ id: "g-prod", name: "Produktion", sortOrder: 0 }];
let emit: (event: api.ConnectEvent) => void = () => undefined;

function seed() {
  const hosts = [web, bastion, config];
  useHostsStore.setState({ hosts, groups, profiles: [withUsage(deploy, 2)], loaded: true });
  vi.mocked(api.hostsList).mockResolvedValue(hosts);
  vi.mocked(api.groupsList).mockResolvedValue(groups);
  vi.mocked(api.profilesList).mockResolvedValue([withUsage(deploy, 2)]);
}

function renderEditor(target: EditorTarget) {
  useUiStore.getState().openModal({ type: "editor", target });
  return render(
    <ToastProvider>
      <EditorModal target={target} />
    </ToastProvider>,
  );
}

const saveButton = () => screen.getByRole("button", { name: "Speichern" });

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  seed();
  vi.mocked(api.newId).mockReturnValue("new-id");
  vi.mocked(api.hostSave).mockImplementation(async (host) => host);
  vi.mocked(api.profileSave).mockImplementation(async (profile) => profile);
  vi.mocked(api.groupSave).mockImplementation(async (group) => group);
  vi.mocked(api.secretSet).mockResolvedValue(undefined);
  vi.mocked(api.connectAnswer).mockResolvedValue(true);
  vi.mocked(api.createEventChannel).mockImplementation((handler) => {
    emit = handler as (event: api.ConnectEvent) => void;
    return {} as never;
  });
});

describe("EditorModal — host", () => {
  it("creates a new host", async () => {
    renderEditor({ kind: "host" });
    expect(screen.getByRole("dialog", { name: "Neuer Host" })).toBeInTheDocument();
    expect(saveButton()).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Anzeigename"), "web-02");
    await userEvent.type(screen.getByLabelText("Adresse (Host oder IP)"), "10.0.0.2");
    await userEvent.selectOptions(screen.getByLabelText("Gruppe"), "g-prod");
    await userEvent.type(screen.getByLabelText("Tags"), "nginx, docker");
    await userEvent.click(screen.getByRole("radio", { name: "Violett" }));
    expect(saveButton()).toBeEnabled();
    await userEvent.click(saveButton());
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
    expect(api.hostSave).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "new-id",
        name: "web-02",
        address: "10.0.0.2",
        port: 22,
        groupId: "g-prod",
        tags: ["nginx", "docker"],
        color: "violet",
        source: "linascp",
        auth: { kind: "custom", auth: { method: "password", keyPath: null, hasSecret: false } },
      }),
    );
    expect(api.secretSet).not.toHaveBeenCalled();
    expect(useHostsStore.getState().selectedId).toBe("new-id");
  });

  it("validates inline and keeps saving disabled while errors exist", async () => {
    renderEditor({ kind: "host", id: "h-web" });
    expect(screen.getByRole("dialog", { name: "Host bearbeiten" })).toBeInTheDocument();
    const port = screen.getByLabelText("Port");
    await userEvent.clear(port);
    await userEvent.type(port, "70000");
    expect(screen.getByText("Port muss zwischen 1 und 65535 liegen")).toBeInTheDocument();
    expect(port).toHaveAttribute("aria-invalid", "true");
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText("Bitte Eingaben prüfen: Allgemein")).toBeInTheDocument();
    await userEvent.clear(port);
    await userEvent.type(port, "2222");
    await userEvent.clear(screen.getByLabelText("Anzeigename"));
    expect(screen.getByText("Pflichtfeld")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Anzeigename"), "web");
    await userEvent.click(screen.getByRole("tab", { name: "Verbindung" }));
    await userEvent.type(screen.getByLabelText("Umgebungsvariablen"), "LANG=de_DE.UTF-8{Enter}kaputt");
    expect(screen.getByText("Zeile 2: Format KEY=VALUE")).toBeInTheDocument();
    const keepalive = screen.getByLabelText("Keepalive-Intervall (s)");
    await userEvent.clear(keepalive);
    await userEvent.type(keepalive, "-1");
    expect(screen.getByText("Ganze Zahl ≥ 0 eingeben")).toBeInTheDocument();
    expect(screen.getByText("Bitte Eingaben prüfen: Verbindung")).toBeInTheDocument();
    expect(saveButton()).toBeDisabled();
  });

  it("stores an individual password in the keyring after saving", async () => {
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("tab", { name: "Authentifizierung" }));
    expect(screen.getByRole("radio", { name: "Individuell" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByLabelText("Benutzer")).toHaveValue("deploy");
    await userEvent.type(screen.getByLabelText("Passwort"), "geheim");
    await userEvent.click(saveButton());
    await waitFor(() => expect(api.secretSet).toHaveBeenCalledWith({ kind: "host", id: "h-web" }, "password", "geheim"));
    expect(api.hostSave).toHaveBeenCalledWith(
      expect.objectContaining({ user: "deploy", auth: { kind: "custom", auth: { method: "password", keyPath: null, hasSecret: true } } }),
    );
  });

  it("creates groups inline, picks a jump host and edits port-forward rules", async () => {
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.selectOptions(screen.getByLabelText("Gruppe"), "__new__");
    await userEvent.type(screen.getByLabelText("Name der neuen Gruppe"), "Kunden");
    await userEvent.click(screen.getByRole("tab", { name: "Verbindung" }));
    const jump = screen.getByLabelText("Jump-Host (ProxyJump)");
    expect(within(jump).queryByRole("option", { name: "web-prod-01" })).not.toBeInTheDocument();
    await userEvent.selectOptions(jump, "h-bastion");
    await userEvent.click(screen.getByRole("button", { name: "+ Regel hinzufügen" }));
    expect(saveButton()).toBeDisabled();
    const rules = screen.getByRole("region", { name: "Port-Forwarding" });
    await userEvent.type(within(rules).getByLabelText("Bind-Adresse"), "8080");
    await userEvent.type(within(rules).getByLabelText("Ziel"), "localhost:80");
    await userEvent.click(within(rules).getByRole("checkbox", { name: "Auto-Start" }));
    await userEvent.click(saveButton());
    await waitFor(() => expect(api.hostSave).toHaveBeenCalled());
    expect(api.groupSave).toHaveBeenCalledWith({ id: "new-id", name: "Kunden", sortOrder: 1 });
    expect(api.hostSave).toHaveBeenCalledWith(
      expect.objectContaining({
        groupId: "new-id",
        jumpHostId: "h-bastion",
        forwards: [{ id: "new-id", kind: "local", bind: "8080", target: "localhost:80", autostart: true }],
      }),
    );
  });

  it("saves an ssh-config host as an overriding LinaSCP host", async () => {
    renderEditor({ kind: "host", id: "h-cfg" });
    expect(screen.getByText(/Dieser Host stammt aus ~\/\.ssh\/config/)).toBeInTheDocument();
    await userEvent.click(saveButton());
    await waitFor(() =>
      expect(api.hostSave).toHaveBeenCalledWith(
        expect.objectContaining({ id: "h-cfg", name: "nas", source: "linascp", color: "accent" }),
      ),
    );
  });
});

describe("EditorModal — login profiles", () => {
  it("navigates to a new profile and back without losing host input", async () => {
    renderEditor({ kind: "host" });
    await userEvent.type(screen.getByLabelText("Anzeigename"), "web-03");
    await userEvent.click(screen.getByRole("tab", { name: "Authentifizierung" }));
    await userEvent.click(screen.getByRole("radio", { name: "Login-Profil verwenden" }));
    await userEvent.click(screen.getByRole("button", { name: "Neues Login-Profil" }));
    const profileDialog = screen.getByRole("dialog", { name: "Neues Login-Profil" });
    expect(profileDialog).toHaveTextContent("Noch nutzt kein Host dieses Profil.");
    await userEvent.type(screen.getByLabelText("Profilname"), "ops");
    await userEvent.click(screen.getByRole("button", { name: "Zurück zum Host" }));
    expect(screen.getByRole("dialog", { name: "Neuer Host" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("radio", { name: "Login-Profil verwenden" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("tab", { name: "Allgemein" }));
    expect(screen.getByLabelText("Anzeigename")).toHaveValue("web-03");
    expect(api.profileSave).not.toHaveBeenCalled();
  });

  it("saves a pushed profile and selects it for the host", async () => {
    vi.mocked(api.newId).mockReturnValueOnce("h-new").mockReturnValueOnce("p-new");
    let profiles = [withUsage(deploy, 2)];
    vi.mocked(api.profileSave).mockImplementation(async (profile) => {
      profiles = [...profiles, withUsage(profile, 0)];
      return profile;
    });
    vi.mocked(api.profilesList).mockImplementation(async () => profiles);
    renderEditor({ kind: "host" });
    await userEvent.type(screen.getByLabelText("Anzeigename"), "web-03");
    await userEvent.type(screen.getByLabelText("Adresse (Host oder IP)"), "10.0.0.3");
    await userEvent.click(screen.getByRole("tab", { name: "Authentifizierung" }));
    await userEvent.click(screen.getByRole("radio", { name: "Login-Profil verwenden" }));
    expect(screen.getByText("Login-Profil auswählen")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neues Login-Profil" }));
    await userEvent.type(screen.getByLabelText("Profilname"), "ops");
    await userEvent.type(screen.getByLabelText("Benutzer"), "root");
    expect(screen.getByRole("radio", { name: "Schlüssel" })).toHaveAttribute("aria-checked", "true");
    await userEvent.type(screen.getByLabelText("Privater Schlüssel"), "~/.ssh/id_ops");
    await userEvent.type(screen.getByLabelText("Passphrase"), "pp-secret");
    await userEvent.click(saveButton());
    expect(await screen.findByRole("dialog", { name: "Neuer Host" })).toBeInTheDocument();
    expect(api.profileSave).toHaveBeenCalledWith(
      expect.objectContaining({ id: "p-new", name: "ops", user: "root", auth: { method: "key", keyPath: "~/.ssh/id_ops", hasSecret: true } }),
    );
    expect(api.secretSet).toHaveBeenCalledWith({ kind: "profile", id: "p-new" }, "passphrase", "pp-secret");
    expect(screen.getByLabelText("Login-Profil")).toHaveValue("p-new");
    await userEvent.click(saveButton());
    await waitFor(() =>
      expect(api.hostSave).toHaveBeenCalledWith(
        expect.objectContaining({ id: "h-new", user: null, auth: { kind: "profile", profileId: "p-new" } }),
      ),
    );
  });

  it("edits an existing login profile", async () => {
    renderEditor({ kind: "profile", id: "p-deploy" });
    const dialog = screen.getByRole("dialog", { name: "Login-Profil bearbeiten" });
    expect(dialog).toHaveTextContent("Änderungen gelten für alle 2 Hosts, die dieses Profil nutzen.");
    expect(screen.queryByRole("button", { name: "Zurück zum Host" })).not.toBeInTheDocument();
    const user = screen.getByLabelText("Benutzer");
    await userEvent.clear(user);
    await userEvent.type(user, "admin");
    await userEvent.click(saveButton());
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
    expect(api.profileSave).toHaveBeenCalledWith(expect.objectContaining({ id: "p-deploy", user: "admin" }));
  });
});

describe("EditorModal — Verbindung testen", () => {
  it("tests the unsaved draft and shows the steps and the result", async () => {
    vi.mocked(api.connectTest).mockImplementation(async () => {
      emit({ kind: "steps", steps: ["tcp", "hostKey", "auth"] });
      for (const step of ["tcp", "hostKey", "auth"] as const) {
        emit({ kind: "stepStarted", step });
        emit({ kind: "stepDone", step });
      }
    });
    renderEditor({ kind: "host", id: "h-web" });
    const name = screen.getByLabelText("Anzeigename");
    await userEvent.clear(name);
    await userEvent.type(name, "web-renamed");
    await userEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
    expect(await screen.findByText("● Verbindung erfolgreich, Anmeldung möglich")).toBeInTheDocument();
    expect(api.connectTest).toHaveBeenCalledWith("new-id", expect.objectContaining({ id: "h-web", name: "web-renamed" }), {});
    expect(screen.getByText("Host-Key prüfen")).toBeInTheDocument();
  });

  it("answers the password prompt from the draft and reports failures", async () => {
    vi.mocked(api.connectTest).mockImplementation(async () => {
      emit({ kind: "steps", steps: ["tcp", "hostKey", "auth"] });
      emit({ kind: "stepStarted", step: "auth" });
      emit({ kind: "prompt", promptId: 7, prompt: { kind: "password", user: "deploy", host: "10.0.1.12", retry: false } });
      emit({ kind: "stepFailed", step: "auth", error: { type: "auth", rejectedMethods: ["password"], message: "denied" } });
      throw { type: "auth", rejectedMethods: ["password"], message: "denied" };
    });
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("tab", { name: "Authentifizierung" }));
    await userEvent.type(screen.getByLabelText("Passwort"), "geheim");
    await userEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
    expect(await screen.findByText("● Anmeldung fehlgeschlagen")).toBeInTheDocument();
    expect(api.connectAnswer).toHaveBeenCalledWith(7, { kind: "secret", value: "geheim", save: false });
  });

  it("asks about unknown host keys inline", async () => {
    let finish: () => void = () => undefined;
    vi.mocked(api.connectTest).mockImplementation(() => {
      emit({ kind: "steps", steps: ["tcp", "hostKey", "auth"] });
      emit({ kind: "stepStarted", step: "hostKey" });
      emit({
        kind: "prompt",
        promptId: 3,
        prompt: { kind: "hostKeyUnknown", host: "10.0.1.12", port: 22, info: { algorithm: "ssh-ed25519", sha256: "SHA256:abc", md5: "MD5:aa" } },
      });
      return new Promise<void>((resolve) => {
        finish = resolve;
      });
    });
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
    expect(await screen.findByText("Unbekannter Host-Key: SHA256:abc")).toBeInTheDocument();
    expect(screen.getByText("Teste Verbindung …")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Vertrauen" }));
    expect(api.connectAnswer).toHaveBeenCalledWith(3, { kind: "hostKey", decision: "trust" });
    expect(screen.queryByText("Unbekannter Host-Key: SHA256:abc")).not.toBeInTheDocument();
    await act(async () => finish());
    expect(screen.getByText("● Verbindung erfolgreich, Anmeldung möglich")).toBeInTheDocument();
  });

  it("cancels a running test when the editor closes", async () => {
    vi.mocked(api.connectTest).mockImplementation(() => new Promise<void>(() => undefined));
    vi.mocked(api.connectCancel).mockResolvedValue(undefined);
    const { unmount } = renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
    unmount();
    expect(api.connectCancel).toHaveBeenCalledWith("new-id");
  });
});
```

**`ui/src/shell/AppShell.test.tsx`**
```tsx
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../components";
import * as api from "../ipc/api";
import { useHostsStore } from "../stores/hosts";
import { DEFAULT_SETTINGS, useSettingsStore } from "../stores/settings";
import { type ModalState, useUiStore } from "../stores/ui";
import { AppShell } from "./AppShell";

vi.mock("../ipc/api");

function mockBackend(settings: Partial<api.Settings> = {}) {
  vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, firstRunDone: true, ...settings });
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
}

function renderShell() {
  return render(
    <ToastProvider>
      <AppShell />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
});

describe("AppShell", () => {
  it("renders the rail and the host overview and loads data", async () => {
    mockBackend();
    renderShell();
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
    await waitFor(() => expect(useSettingsStore.getState().loaded).toBe(true));
    await waitFor(() => expect(useHostsStore.getState().loaded).toBe(true));
  });

  it("switches views via the rail and keeps inactive views mounted", async () => {
    mockBackend();
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Server-Übersicht" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht", hidden: true })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "SFTP" }));
    expect(screen.getByRole("heading", { name: "SFTP" })).toBeInTheDocument();
    expect(screen.getByText("Der SFTP-Dateimanager folgt in einer späteren Version.")).toBeVisible();
  });

  it("applies the persisted theme", async () => {
    mockBackend({ theme: "light" });
    renderShell();
    await waitFor(() => expect(document.documentElement.classList.contains("light")).toBe(true));
  });

  it("opens the first-run dialog while the first run is not done", async () => {
    mockBackend({ firstRunDone: false });
    renderShell();
    expect(await screen.findByRole("dialog", { name: "Willkommen bei LinaSCP" })).toBeInTheDocument();
  });

  it("renders every modal type", async () => {
    mockBackend();
    renderShell();
    const cases: Array<[ModalState, string]> = [
      [{ type: "editor", target: { kind: "host" } }, "Neuer Host"],
      [{ type: "editor", target: { kind: "profile", id: "p1" } }, "Neues Login-Profil"],
      [{ type: "settings" }, "Einstellungen"],
      [{ type: "firstRun" }, "Willkommen bei LinaSCP"],
      [{ type: "export" }, "Exportieren"],
      [{ type: "import" }, "Importieren"],
      [{ type: "connect" }, "Verbindung"],
    ];
    for (const [modal, title] of cases) {
      act(() => useUiStore.getState().openModal(modal));
      expect(screen.getByRole("dialog", { name: title })).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reports load failures as an error toast", async () => {
    mockBackend();
    vi.mocked(api.settingsGet).mockRejectedValue({ type: "store", message: "database is locked" });
    renderShell();
    expect(await screen.findByRole("alert")).toHaveTextContent("Daten konnten nicht geladen werden");
  });
});
```

- [ ] **Step 5: Run the editor tests to verify they fail**

Run: `bun run test EditorModal AppShell`
Expected: FAIL — the stub renders "Host bearbeiten" with "Folgt in einer späteren Version" (no "Neuer Host" dialog, no fields).

- [ ] **Step 6: Implement the editor**

**`ui/src/screens/editor/form.ts`**
```ts
import type { HostDraft } from "./drafts";
import type { HostField } from "./validate";

/** Props shared by the host editor tabs. */
export interface HostFormProps {
  draft: HostDraft;
  /** Applies a patch; `field` marks the field as touched so its error becomes visible. */
  edit(patch: Partial<HostDraft>, field?: HostField): void;
  /** Error of a touched field. */
  fieldError(field: HostField): string | undefined;
}
```

**`ui/src/screens/editor/secrets.ts`**
```ts
import { secretSet, type SecretOwner } from "../../ipc/api";
import type { AuthDraft } from "./drafts";

/** Writes entered secrets to the keyring after the owner was saved. Returns `false` if the keyring refused. */
export async function storeSecrets(owner: SecretOwner, auth: AuthDraft): Promise<boolean> {
  try {
    if (auth.method === "password" && auth.password) await secretSet(owner, "password", auth.password);
    if (auth.method === "key" && auth.passphrase) await secretSet(owner, "passphrase", auth.passphrase);
    return true;
  } catch {
    return false;
  }
}
```

**`ui/src/screens/editor/useConnectionTest.ts`**
```ts
import { useCallback, useEffect, useRef, useState } from "react";
import {
  type ConnectEvent,
  type CoreError,
  connectAnswer,
  connectCancel,
  connectTest,
  createEventChannel,
  type Host,
  newId,
  type Prompt,
  type PromptAnswer,
} from "../../ipc/api";
import { applyConnectEvent, asCoreError, EMPTY_PROGRESS, fireAndForget, type StepProgress } from "../../stores/connectProgress";
import type { AuthDraft } from "./drafts";

export type TestStatus = "idle" | "running" | "ok" | "failed";

export interface TestState {
  status: TestStatus;
  progress: StepProgress;
  prompt?: { promptId: number; prompt: Prompt };
  error?: CoreError;
}

const IDLE: TestState = { status: "idle", progress: EMPTY_PROGRESS };

/** Secrets typed into the (unsaved) draft answer the first password/passphrase prompt of a test. */
export function autoAnswer(prompt: Prompt, auth: AuthDraft | null): PromptAnswer | null {
  if (!auth) return null;
  if (prompt.kind === "password" && !prompt.retry && auth.password) return { kind: "secret", value: auth.password, save: false };
  if (prompt.kind === "passphrase" && auth.passphrase) return { kind: "secret", value: auth.passphrase, save: false };
  return null;
}

/** "Verbindung testen": runs `connectTest` for an unsaved host; cancels a running test on unmount. */
export function useConnectionTest() {
  const [state, setState] = useState<TestState>(IDLE);
  const running = useRef<string | null>(null);

  useEffect(
    () => () => {
      const connectId = running.current;
      running.current = null;
      if (connectId) fireAndForget(() => connectCancel(connectId));
    },
    [],
  );

  const run = useCallback(async (host: Host, auth: AuthDraft | null) => {
    const connectId = newId();
    running.current = connectId;
    setState({ status: "running", progress: EMPTY_PROGRESS });
    const events = createEventChannel<ConnectEvent>((event) => {
      if (running.current !== connectId) return;
      if (event.kind === "prompt") {
        const answer = autoAnswer(event.prompt, auth);
        if (answer) {
          fireAndForget(() => connectAnswer(event.promptId, answer));
          return;
        }
        setState((current) => ({ ...current, prompt: { promptId: event.promptId, prompt: event.prompt } }));
        return;
      }
      setState((current) => ({ ...current, progress: applyConnectEvent(current.progress, event) }));
    });
    try {
      await connectTest(connectId, host, events);
      if (running.current === connectId) setState((current) => ({ ...current, status: "ok", prompt: undefined }));
    } catch (error) {
      if (running.current === connectId) {
        setState((current) => ({ ...current, status: "failed", error: asCoreError(error), prompt: undefined }));
      }
    } finally {
      if (running.current === connectId) running.current = null;
    }
  }, []);

  const answer = useCallback((promptId: number, value: PromptAnswer) => {
    setState((current) => ({ ...current, prompt: undefined }));
    fireAndForget(() => connectAnswer(promptId, value));
  }, []);

  const cancel = useCallback(() => {
    const connectId = running.current;
    running.current = null;
    setState(IDLE);
    if (connectId) fireAndForget(() => connectCancel(connectId));
  }, []);

  return { state, run, answer, cancel };
}
```

**`ui/src/screens/editor/ConnectionTest.tsx`**
```tsx
import { Zap } from "lucide-react";
import { Button, cx, StepList } from "../../components";
import { t } from "../../i18n/de";
import type { PromptAnswer } from "../../ipc/api";
import { isInlinePrompt, PromptForm } from "../connect/PromptForm";
import { errorTitle, stepItems } from "../connect/steps";
import styles from "./Editor.module.css";
import type { TestState } from "./useConnectionTest";

/** Footer part: "Verbindung testen" + one-line result. */
export function ConnectionTestStatus({
  state,
  disabled,
  onRun,
  onCancel,
}: {
  state: TestState;
  disabled: boolean;
  onRun: () => void;
  onCancel: () => void;
}) {
  const running = state.status === "running";
  const text =
    state.status === "running"
      ? t.editor.testRunning
      : state.status === "ok"
        ? t.editor.testOk
        : state.status === "failed" && state.error
          ? t.editor.testFailed(errorTitle(state.error))
          : "";
  return (
    <>
      {running ? (
        <Button variant="ghost" onClick={onCancel}>
          {t.editor.testCancel}
        </Button>
      ) : (
        <Button icon={Zap} disabled={disabled} onClick={onRun}>
          {t.editor.test}
        </Button>
      )}
      <span
        role="status"
        className={cx(styles.status, state.status === "ok" && styles.statusOk, state.status === "failed" && styles.statusFailed)}
      >
        {text}
      </span>
    </>
  );
}

/** Body part: steps of the running/finished test and inline prompts. */
export function ConnectionTestPanel({
  state,
  jumpName,
  onAnswer,
}: {
  state: TestState;
  jumpName?: string;
  onAnswer: (promptId: number, answer: PromptAnswer) => void;
}) {
  if (state.status === "idle") return null;
  const pending = state.prompt;
  let detail = null;
  if (pending) {
    const { promptId, prompt } = pending;
    if (prompt.kind === "hostKeyUnknown") {
      detail = (
        <div className={styles.hostKey}>
          <span className="mono">{t.editor.hostKeyUnknown(prompt.info.sha256)}</span>
          <Button size="sm" variant="ghost" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "reject" })}>
            {t.common.cancel}
          </Button>
          <Button size="sm" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "acceptOnce" })}>
            {t.editor.acceptOnce}
          </Button>
          <Button size="sm" variant="primary" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "trust" })}>
            {t.editor.trust}
          </Button>
        </div>
      );
    } else if (prompt.kind === "hostKeyChanged") {
      detail = (
        <div className={styles.hostKey}>
          <span>{t.editor.hostKeyChanged}</span>
          <Button size="sm" variant="inverted" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "reject" })}>
            {t.common.cancel}
          </Button>
        </div>
      );
    } else if (isInlinePrompt(prompt)) {
      detail = <PromptForm prompt={prompt} showSave={false} onAnswer={(answer) => onAnswer(promptId, answer)} />;
    }
  }
  return (
    <section className={styles.test} aria-label={t.editor.testSteps}>
      <StepList steps={stepItems(state.progress, { jumpName, detail })} />
      {detail && !state.progress.current ? detail : null}
    </section>
  );
}
```

**`ui/src/screens/editor/ColorSwatches.tsx`**
```tsx
import { type KeyboardEvent, useRef } from "react";
import { cx, nextIndex } from "../../components";
import { t } from "../../i18n/de";
import { HOST_COLORS, type HostColor } from "../../model/hostFormat";
import styles from "./Editor.module.css";

export function ColorSwatches({ value, onChange }: { value: HostColor; onChange: (color: HostColor) => void }) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, HOST_COLORS.length, "horizontal");
    const color = next === null ? undefined : HOST_COLORS[next];
    if (next === null || !color) return;
    event.preventDefault();
    onChange(color);
    refs.current[next]?.focus();
  };

  return (
    <div role="radiogroup" aria-label={t.editor.color} className={styles.swatches}>
      {HOST_COLORS.map((color, index) => {
        const checked = color === value;
        return (
          <button
            key={color}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={t.editor.colors[color]}
            title={t.editor.colors[color]}
            tabIndex={checked ? 0 : -1}
            className={cx(styles.swatch, checked && styles.swatchChecked)}
            style={{ background: `var(--${color})` }}
            onClick={() => onChange(color)}
            onKeyDown={(event) => onKeyDown(event, index)}
          />
        );
      })}
    </div>
  );
}
```

**`ui/src/screens/editor/GeneralTab.tsx`**
```tsx
import { Checkbox, cx, Select, TextArea, TextField } from "../../components";
import { t } from "../../i18n/de";
import { useHostsStore } from "../../stores/hosts";
import { ColorSwatches } from "./ColorSwatches";
import { NEW_GROUP, NO_GROUP } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";

export function GeneralTab({ draft, edit, fieldError }: HostFormProps) {
  const groups = useHostsStore((state) => state.groups);
  const groupOptions = [
    { value: NO_GROUP, label: t.editor.noGroup },
    ...groups.map((group) => ({ value: group.id, label: group.name })),
    { value: NEW_GROUP, label: t.editor.newGroup },
  ];
  return (
    <div className={styles.grid}>
      {draft.source === "sshConfig" ? <p className={cx(styles.banner, styles.wide)}>{t.editor.sshConfigHint}</p> : null}
      <TextField
        className={styles.wide}
        label={t.editor.name}
        value={draft.name}
        onChange={(name) => edit({ name }, "name")}
        error={fieldError("name")}
      />
      <TextField
        label={t.editor.address}
        mono
        spellCheck={false}
        value={draft.address}
        onChange={(address) => edit({ address }, "address")}
        error={fieldError("address")}
      />
      <TextField
        label={t.editor.port}
        mono
        inputMode="numeric"
        value={draft.port}
        onChange={(port) => edit({ port }, "port")}
        error={fieldError("port")}
      />
      <Select label={t.editor.group} value={draft.groupId} options={groupOptions} onChange={(groupId) => edit({ groupId })} />
      <TextField label={t.editor.tags} hint={t.editor.tagsHint} value={draft.tags} onChange={(tags) => edit({ tags })} />
      {draft.groupId === NEW_GROUP ? (
        <TextField
          label={t.editor.newGroupName}
          value={draft.newGroupName}
          onChange={(newGroupName) => edit({ newGroupName }, "newGroupName")}
          error={fieldError("newGroupName")}
        />
      ) : null}
      <fieldset className={cx(styles.fieldset, styles.wide)}>
        <legend className={styles.legend}>{t.editor.protocols}</legend>
        <div className={styles.row}>
          <Checkbox label={t.editor.protocolSsh} checked={draft.protocolSsh} onChange={(protocolSsh) => edit({ protocolSsh })} />
          <Checkbox label={t.editor.protocolSftp} checked={draft.protocolSftp} onChange={(protocolSftp) => edit({ protocolSftp })} />
        </div>
      </fieldset>
      <div className={cx(styles.fieldset, styles.wide)}>
        <span className={styles.legend} aria-hidden="true">
          {t.editor.color}
        </span>
        <ColorSwatches value={draft.color} onChange={(color) => edit({ color })} />
      </div>
      <TextArea className={styles.wide} label={t.editor.notes} value={draft.notes} onChange={(notes) => edit({ notes })} />
    </div>
  );
}
```

**`ui/src/screens/editor/AuthMethodFields.tsx`**
```tsx
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { Button, Segmented, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { AuthMethod } from "../../ipc/api";
import type { AuthDraft } from "./drafts";
import styles from "./Editor.module.css";

const METHOD_OPTIONS: ReadonlyArray<{ value: AuthMethod; label: string }> = [
  { value: "password", label: t.auth.password },
  { value: "key", label: t.auth.key },
  { value: "agent", label: t.auth.agent },
  { value: "keyboardInteractive", label: t.auth.keyboardInteractive },
];

/** Method switch + method specific fields (shared by individual host auth and login profiles). */
export function AuthMethodFields({
  auth,
  onChange,
  keyPathError,
}: {
  auth: AuthDraft;
  onChange: (auth: AuthDraft) => void;
  keyPathError?: string;
}) {
  const set = (patch: Partial<AuthDraft>) => onChange({ ...auth, ...patch });

  const pickKey = async () => {
    const path = await open({ multiple: false, directory: false, title: t.editor.pickKeyTitle });
    if (typeof path === "string") set({ keyPath: path });
  };

  return (
    <>
      <div className={styles.fieldset}>
        <span className={styles.legend} aria-hidden="true">
          {t.editor.method}
        </span>
        <Segmented label={t.editor.method} value={auth.method} options={METHOD_OPTIONS} onChange={(method) => set({ method })} />
      </div>
      {auth.method === "password" ? (
        <TextField
          type="password"
          label={t.editor.password}
          autoComplete="new-password"
          placeholder={auth.hasSecret ? t.editor.secretStored : undefined}
          hint={t.editor.secretHint}
          value={auth.password}
          onChange={(password) => set({ password })}
        />
      ) : null}
      {auth.method === "key" ? (
        <>
          <div className={styles.inline}>
            <TextField
              label={t.editor.keyPath}
              mono
              spellCheck={false}
              placeholder={t.editor.keyPathPlaceholder}
              value={auth.keyPath}
              error={keyPathError}
              onChange={(keyPath) => set({ keyPath })}
            />
            <Button icon={FolderOpen} onClick={() => void pickKey()}>
              {t.editor.pickKey}
            </Button>
          </div>
          <TextField
            type="password"
            label={t.editor.passphrase}
            autoComplete="new-password"
            placeholder={auth.hasSecret ? t.editor.secretStored : t.editor.passphrasePlaceholder}
            hint={t.editor.secretHint}
            value={auth.passphrase}
            onChange={(passphrase) => set({ passphrase })}
          />
        </>
      ) : null}
      {auth.method === "agent" ? <p className={styles.info}>{t.editor.agentInfo}</p> : null}
      {auth.method === "keyboardInteractive" ? <p className={styles.info}>{t.editor.kbdInfo}</p> : null}
    </>
  );
}
```

**`ui/src/screens/editor/AuthTab.tsx`**
```tsx
import { Pencil, Plus } from "lucide-react";
import { Button, OptionCards, Select, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { Id } from "../../ipc/api";
import { methodLabel } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { AuthMethodFields } from "./AuthMethodFields";
import type { AuthSource } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";

const SOURCE_OPTIONS: ReadonlyArray<{ value: AuthSource; label: string; hint: string }> = [
  { value: "profile", label: t.editor.useProfile, hint: t.editor.useProfileHint },
  { value: "custom", label: t.editor.custom, hint: t.editor.customHint },
];

export function AuthTab({ draft, edit, fieldError, onEditProfile }: HostFormProps & { onEditProfile: (id?: Id) => void }) {
  const profiles = useHostsStore((state) => state.profiles);
  const selected = profiles.find((entry) => entry.profile.id === draft.profileId);
  const profileOptions = [
    { value: "", label: t.editor.chooseProfile, disabled: true },
    ...profiles.map(({ profile }) => ({ value: profile.id, label: `${profile.name} · ${methodLabel(profile.auth.method)}` })),
  ];

  return (
    <div className={styles.stack}>
      <OptionCards
        label={t.editor.credentials}
        value={draft.authSource}
        options={SOURCE_OPTIONS}
        onChange={(authSource) => edit({ authSource }, "profileId")}
      />
      {draft.authSource === "profile" ? (
        <>
          {profiles.length === 0 ? (
            <p className={styles.banner}>{t.editor.noProfiles}</p>
          ) : (
            <Select
              label={t.editor.profile}
              value={draft.profileId}
              options={profileOptions}
              onChange={(profileId) => edit({ profileId }, "profileId")}
              error={fieldError("profileId")}
            />
          )}
          {profiles.length === 0 && fieldError("profileId") ? <p className={styles.errorText}>{fieldError("profileId")}</p> : null}
          {selected ? (
            <dl className={styles.summary}>
              <dt>{t.editor.user}</dt>
              <dd className="mono">{selected.profile.user}</dd>
              <dt>{t.editor.method}</dt>
              <dd>{methodLabel(selected.profile.auth.method)}</dd>
              <dt>{t.editor.key}</dt>
              <dd className="mono">{selected.profile.auth.keyPath ?? t.hosts.none}</dd>
              <dt>{t.editor.usedBy}</dt>
              <dd>{t.editor.hostCount(selected.usedBy)}</dd>
            </dl>
          ) : null}
          <div className={styles.row}>
            <Button icon={Pencil} disabled={!selected} onClick={() => onEditProfile(draft.profileId)}>
              {t.editor.editProfileButton}
            </Button>
            <Button icon={Plus} onClick={() => onEditProfile()}>
              {t.editor.newProfileButton}
            </Button>
          </div>
        </>
      ) : (
        <>
          <TextField label={t.editor.user} mono spellCheck={false} value={draft.user} onChange={(user) => edit({ user })} />
          <AuthMethodFields auth={draft.auth} onChange={(auth) => edit({ auth }, "keyPath")} keyPathError={fieldError("keyPath")} />
        </>
      )}
    </div>
  );
}
```

**`ui/src/screens/editor/ForwardsEditor.tsx`**
```tsx
import { Trash2 } from "lucide-react";
import { useId } from "react";
import { Checkbox, cx, IconButton, Select, TextField } from "../../components";
import { t } from "../../i18n/de";
import { type ForwardKind, newId, type PortForward } from "../../ipc/api";
import styles from "./Editor.module.css";

const KIND_OPTIONS: ReadonlyArray<{ value: ForwardKind; label: string }> = [
  { value: "local", label: t.editor.forwardKinds.local },
  { value: "remote", label: t.editor.forwardKinds.remote },
  { value: "dynamic", label: t.editor.forwardKinds.dynamic },
];

/** Port-forward rules are stored with the host; they take effect in TP5. */
export function ForwardsEditor({
  forwards,
  onChange,
  error,
  className,
}: {
  forwards: PortForward[];
  onChange: (forwards: PortForward[]) => void;
  error?: string;
  className?: string;
}) {
  const headingId = useId();
  const update = (index: number, patch: Partial<PortForward>) =>
    onChange(forwards.map((forward, i) => (i === index ? { ...forward, ...patch } : forward)));

  return (
    <section className={cx(styles.stack, className)} aria-labelledby={headingId}>
      <div>
        <h3 id={headingId} className={styles.sectionLabel}>
          {t.editor.portForwarding}
        </h3>
        <p className={styles.hintText}>{t.editor.forwardsHint}</p>
      </div>
      {forwards.length > 0 ? (
        <ul className={styles.forwardList}>
          {forwards.map((forward, index) => (
            <li key={forward.id} className={styles.forwardRow}>
              <Select label={t.editor.forwardKind} value={forward.kind} options={KIND_OPTIONS} onChange={(kind) => update(index, { kind })} />
              <TextField
                label={t.editor.forwardBind}
                mono
                placeholder={t.editor.forwardBindPlaceholder}
                value={forward.bind}
                onChange={(bind) => update(index, { bind })}
              />
              <TextField
                label={t.editor.forwardTarget}
                mono
                placeholder={t.editor.forwardTargetPlaceholder}
                disabled={forward.kind === "dynamic"}
                value={forward.kind === "dynamic" ? "" : forward.target}
                onChange={(target) => update(index, { target })}
              />
              <Checkbox label={t.editor.autostart} checked={forward.autostart} onChange={(autostart) => update(index, { autostart })} />
              <IconButton
                label={t.editor.removeForward}
                icon={Trash2}
                size="sm"
                onClick={() => onChange(forwards.filter((_, i) => i !== index))}
              />
            </li>
          ))}
        </ul>
      ) : null}
      {error ? <p className={styles.errorText}>{error}</p> : null}
      <button
        type="button"
        className={styles.addButton}
        onClick={() => onChange([...forwards, { id: newId(), kind: "local", bind: "", target: "", autostart: false }])}
      >
        {t.editor.addForward}
      </button>
    </section>
  );
}
```

**`ui/src/screens/editor/ConnectionTab.tsx`**
```tsx
import { Select, TextArea, TextField } from "../../components";
import { t } from "../../i18n/de";
import { useHostsStore } from "../../stores/hosts";
import { NO_JUMP } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";
import { ForwardsEditor } from "./ForwardsEditor";

export function ConnectionTab({ draft, edit, fieldError }: HostFormProps) {
  const hosts = useHostsStore((state) => state.hosts);
  const jumpOptions = [
    { value: NO_JUMP, label: t.editor.noJump },
    ...hosts.filter((host) => host.id !== draft.id).map((host) => ({ value: host.id, label: host.name })),
  ];
  return (
    <div className={styles.grid}>
      <Select
        className={styles.wide}
        label={t.editor.jumpHost}
        value={draft.jumpHostId}
        options={jumpOptions}
        onChange={(jumpHostId) => edit({ jumpHostId })}
      />
      <TextField
        label={t.editor.keepalive}
        mono
        inputMode="numeric"
        value={draft.keepalive}
        onChange={(keepalive) => edit({ keepalive }, "keepalive")}
        error={fieldError("keepalive")}
      />
      <TextField
        label={t.editor.timeout}
        mono
        inputMode="numeric"
        value={draft.timeout}
        onChange={(timeout) => edit({ timeout }, "timeout")}
        error={fieldError("timeout")}
      />
      <TextField
        className={styles.wide}
        label={t.editor.startCommand}
        mono
        spellCheck={false}
        placeholder={t.editor.startCommandPlaceholder}
        value={draft.startCommand}
        onChange={(startCommand) => edit({ startCommand })}
      />
      <TextArea
        className={styles.wide}
        label={t.editor.env}
        mono
        spellCheck={false}
        hint={t.editor.envHint}
        value={draft.env}
        onChange={(env) => edit({ env }, "env")}
        error={fieldError("env")}
      />
      <ForwardsEditor
        className={styles.wide}
        forwards={draft.forwards}
        onChange={(forwards) => edit({ forwards }, "forwards")}
        error={fieldError("forwards")}
      />
    </div>
  );
}
```

**`ui/src/screens/editor/SftpTab.tsx`**
```tsx
import { cx, Select, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { TransferMode } from "../../ipc/api";
import type { SftpDraft } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";

const MODE_OPTIONS: ReadonlyArray<{ value: TransferMode; label: string }> = [
  { value: "auto", label: t.editor.transferModes.auto },
  { value: "binary", label: t.editor.transferModes.binary },
  { value: "text", label: t.editor.transferModes.text },
];

/** Stored with the host; used by the SFTP file manager from TP3 on. */
export function SftpTab({ draft, edit, fieldError }: HostFormProps) {
  const set = (patch: Partial<SftpDraft>, field?: "parallel" | "bandwidthLimit") => edit({ sftp: { ...draft.sftp, ...patch } }, field);
  return (
    <div className={styles.grid}>
      <p className={cx(styles.banner, styles.wide)}>{t.editor.sftpHint}</p>
      <TextField
        label={t.editor.remoteStart}
        mono
        spellCheck={false}
        value={draft.sftp.remoteStart}
        onChange={(remoteStart) => set({ remoteStart })}
      />
      <TextField
        label={t.editor.localStart}
        mono
        spellCheck={false}
        value={draft.sftp.localStart}
        onChange={(localStart) => set({ localStart })}
      />
      <Select
        label={t.editor.transferMode}
        value={draft.sftp.transferMode}
        options={MODE_OPTIONS}
        onChange={(transferMode) => set({ transferMode })}
      />
      <TextField
        label={t.editor.parallel}
        mono
        inputMode="numeric"
        value={draft.sftp.parallel}
        onChange={(parallel) => set({ parallel }, "parallel")}
        error={fieldError("parallel")}
      />
      <TextField
        label={t.editor.uploadPerms}
        mono
        placeholder={t.editor.uploadPermsPlaceholder}
        value={draft.sftp.uploadPerms}
        onChange={(uploadPerms) => set({ uploadPerms })}
      />
      <TextField
        label={t.editor.bandwidth}
        mono
        inputMode="numeric"
        placeholder={t.editor.bandwidthPlaceholder}
        value={draft.sftp.bandwidthLimit}
        onChange={(bandwidthLimit) => set({ bandwidthLimit }, "bandwidthLimit")}
        error={fieldError("bandwidthLimit")}
      />
      <TextField
        className={styles.wide}
        label={t.editor.sftpServer}
        mono
        spellCheck={false}
        value={draft.sftp.sftpServerCmd}
        onChange={(sftpServerCmd) => set({ sftpServerCmd })}
      />
    </div>
  );
}
```

**`ui/src/screens/editor/HostEditor.tsx`**
```tsx
import { useId, useMemo, useState } from "react";
import { Button, Dialog, Tabs, tabId, tabPanelId, useToast } from "../../components";
import { t } from "../../i18n/de";
import { type Id, newId } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { formatEndpoint, hostColorVar, initials } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { AuthTab } from "./AuthTab";
import { ConnectionTab } from "./ConnectionTab";
import { ConnectionTestPanel, ConnectionTestStatus } from "./ConnectionTest";
import { draftGroupId, draftToHost, type EditorTab, type HostDraft, NEW_GROUP } from "./drafts";
import type { HostFrame } from "./EditorModal";
import styles from "./Editor.module.css";
import { GeneralTab } from "./GeneralTab";
import { storeSecrets } from "./secrets";
import { SftpTab } from "./SftpTab";
import { useConnectionTest } from "./useConnectionTest";
import { errorTabs, type HostField, hasErrors, validateHost } from "./validate";

const TAB_ORDER: readonly EditorTab[] = ["general", "auth", "connection", "sftp"];
const TABS = TAB_ORDER.map((id) => ({ id, label: t.editor.tabs[id] }));

export function HostEditor({
  frame,
  onChange,
  onEditProfile,
}: {
  frame: HostFrame;
  onChange: (frame: HostFrame) => void;
  onEditProfile: (profileId?: Id) => void;
}) {
  const { draft, tab, isNew } = frame;
  const groups = useHostsStore((state) => state.groups);
  const hosts = useHostsStore((state) => state.hosts);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const test = useConnectionTest();
  const idBase = useId();
  const [touched, setTouched] = useState<ReadonlySet<HostField>>(() => new Set());
  const [saving, setSaving] = useState(false);
  const errors = useMemo(() => validateHost(draft), [draft]);
  const invalid = hasErrors(errors);

  const edit = (patch: Partial<HostDraft>, field?: HostField) => {
    onChange({ ...frame, draft: { ...draft, ...patch } });
    if (field) setTouched((previous) => new Set(previous).add(field));
  };
  const fieldError = (field: HostField) => (touched.has(field) ? errors[field] : undefined);

  const save = async () => {
    if (invalid) return;
    setSaving(true);
    try {
      const store = useHostsStore.getState();
      let groupId = draftGroupId(draft);
      if (draft.groupId === NEW_GROUP) {
        const group = await store.saveGroup({ id: newId(), name: draft.newGroupName.trim(), sortOrder: store.groups.length });
        groupId = group.id;
      }
      const saved = await store.saveHost(draftToHost(draft, groupId));
      if (draft.authSource === "custom" && !(await storeSecrets({ kind: "host", id: saved.id }, draft.auth))) {
        toast.show({ tone: "amber", message: t.editor.secretFailed });
      }
      useHostsStore.getState().select(saved.id);
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    } finally {
      setSaving(false);
    }
  };

  const runTest = () => void test.run(draftToHost(draft, draftGroupId(draft)), draft.authSource === "custom" ? draft.auth : null);

  const groupName = groups.find((group) => group.id === draft.groupId)?.name ?? t.hosts.noGroup;
  const address = draft.address.trim();
  const user = draft.authSource === "custom" ? draft.user.trim() : "";
  const subtitle = address ? t.editor.hostSubtitle(formatEndpoint(user, address, Number(draft.port) || 22), groupName) : groupName;
  const jumpName = hosts.find((host) => host.id === draft.jumpHostId)?.name;
  const invalidTabs = errorTabs(errors).map((id) => t.editor.tabs[id]).join(", ");

  return (
    <Dialog
      open
      size="lg"
      title={isNew ? t.editor.newHost : t.editor.editHost}
      subtitle={subtitle}
      onClose={closeModal}
      icon={
        <span className={styles.headIcon} style={{ color: hostColorVar(draft.color) }} aria-hidden="true">
          {initials(draft.name)}
        </span>
      }
      footer={
        <>
          <ConnectionTestStatus state={test.state} disabled={invalid} onRun={runTest} onCancel={test.cancel} />
          <span className={styles.spacer} />
          {invalid ? <span className={styles.invalidHint}>{t.editor.invalidHint(invalidTabs)}</span> : null}
          <Button variant="ghost" onClick={closeModal}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" disabled={invalid || saving} onClick={() => void save()}>
            {t.common.save}
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        <div className={styles.layout}>
          <Tabs
            label={t.editor.sections}
            tabs={TABS}
            value={tab}
            onChange={(id) => onChange({ ...frame, tab: id as EditorTab })}
            orientation="vertical"
            idBase={idBase}
            className={styles.tabs}
          />
          <div role="tabpanel" id={tabPanelId(idBase, tab)} aria-labelledby={tabId(idBase, tab)} className={styles.panel}>
            {tab === "general" ? <GeneralTab draft={draft} edit={edit} fieldError={fieldError} /> : null}
            {tab === "auth" ? <AuthTab draft={draft} edit={edit} fieldError={fieldError} onEditProfile={onEditProfile} /> : null}
            {tab === "connection" ? <ConnectionTab draft={draft} edit={edit} fieldError={fieldError} /> : null}
            {tab === "sftp" ? <SftpTab draft={draft} edit={edit} fieldError={fieldError} /> : null}
          </div>
        </div>
        <ConnectionTestPanel state={test.state} jumpName={jumpName} onAnswer={test.answer} />
      </div>
    </Dialog>
  );
}
```

**`ui/src/screens/editor/ProfileEditor.tsx`**
```tsx
import { ArrowLeft } from "lucide-react";
import { useMemo, useState } from "react";
import { Button, cx, Dialog, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { LoginProfile } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { initials } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { AuthMethodFields } from "./AuthMethodFields";
import { draftToProfile, type ProfileDraft } from "./drafts";
import type { ProfileFrame } from "./EditorModal";
import styles from "./Editor.module.css";
import { storeSecrets } from "./secrets";
import { hasErrors, type ProfileField, validateProfile } from "./validate";

export function ProfileEditor({
  frame,
  onChange,
  onBack,
  onSaved,
}: {
  frame: ProfileFrame;
  onChange: (frame: ProfileFrame) => void;
  /** Set when the profile was opened from a host ("Zurück zum Host"). */
  onBack?: () => void;
  onSaved: (profile: LoginProfile) => void;
}) {
  const { draft, isNew } = frame;
  const usedBy = useHostsStore((state) => state.profiles.find((entry) => entry.profile.id === draft.id)?.usedBy ?? 0);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [touched, setTouched] = useState<ReadonlySet<ProfileField>>(() => new Set());
  const [saving, setSaving] = useState(false);
  const errors = useMemo(() => validateProfile(draft), [draft]);
  const invalid = hasErrors(errors);

  const edit = (patch: Partial<ProfileDraft>, field?: ProfileField) => {
    onChange({ ...frame, draft: { ...draft, ...patch } });
    if (field) setTouched((previous) => new Set(previous).add(field));
  };
  const fieldError = (field: ProfileField) => (touched.has(field) ? errors[field] : undefined);

  const save = async () => {
    if (invalid) return;
    setSaving(true);
    try {
      const saved = await useHostsStore.getState().saveProfile(draftToProfile(draft));
      if (!(await storeSecrets({ kind: "profile", id: saved.id }, draft.auth))) {
        toast.show({ tone: "amber", message: t.editor.secretFailed });
      }
      onSaved(saved);
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      size="lg"
      title={isNew ? t.editor.newProfile : t.editor.editProfile}
      subtitle={t.editor.profileSubtitle(draft.user.trim() || t.hosts.none, usedBy)}
      onClose={closeModal}
      icon={
        <span className={cx(styles.headIcon, styles.profileIcon)} aria-hidden="true">
          {initials(draft.name)}
        </span>
      }
      footer={
        <>
          {onBack ? (
            <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
              {t.editor.backToHost}
            </Button>
          ) : null}
          <span className={styles.spacer} />
          <Button variant="ghost" onClick={closeModal}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" disabled={invalid || saving} onClick={() => void save()}>
            {t.common.save}
          </Button>
        </>
      }
    >
      <div className={styles.stack}>
        <p className={styles.banner}>{t.editor.profileInfo(usedBy)}</p>
        <div className={styles.grid}>
          <TextField
            label={t.editor.profileName}
            value={draft.name}
            onChange={(name) => edit({ name }, "name")}
            error={fieldError("name")}
          />
          <TextField
            label={t.editor.user}
            mono
            spellCheck={false}
            value={draft.user}
            onChange={(user) => edit({ user }, "user")}
            error={fieldError("user")}
          />
        </div>
        <AuthMethodFields auth={draft.auth} onChange={(auth) => edit({ auth }, "keyPath")} keyPathError={fieldError("keyPath")} />
      </div>
    </Dialog>
  );
}
```

**`ui/src/screens/editor/EditorModal.tsx`**
```tsx
import { useState } from "react";
import type { Id } from "../../ipc/api";
import { newHost, newProfile } from "../../model/defaults";
import { useHostsStore } from "../../stores/hosts";
import { type EditorTarget, useUiStore } from "../../stores/ui";
import { type EditorTab, type HostDraft, hostToDraft, type ProfileDraft, profileToDraft } from "./drafts";
import { HostEditor } from "./HostEditor";
import { ProfileEditor } from "./ProfileEditor";

export interface HostFrame {
  kind: "host";
  draft: HostDraft;
  tab: EditorTab;
  isNew: boolean;
}

export interface ProfileFrame {
  kind: "profile";
  draft: ProfileDraft;
  isNew: boolean;
}

export type EditorFrame = HostFrame | ProfileFrame;

function hostFrame(id?: Id): HostFrame {
  const host = id === undefined ? undefined : useHostsStore.getState().hosts.find((h) => h.id === id);
  return { kind: "host", draft: hostToDraft(host ?? newHost({ color: "accent" })), tab: "general", isNew: !host };
}

function profileFrame(id?: Id): ProfileFrame {
  const profile = id === undefined ? undefined : useHostsStore.getState().profiles.find((p) => p.profile.id === id)?.profile;
  return { kind: "profile", draft: profileToDraft(profile ?? newProfile()), isNew: !profile };
}

/**
 * Host ↔ login-profile editor. Frames form a navigation stack: a profile opened from the host's
 * auth tab is pushed on top; "Zurück zum Host" pops it and the unsaved host draft reappears.
 */
export function EditorModal({ target }: { target: EditorTarget }) {
  const [stack, setStack] = useState<EditorFrame[]>(() => [target.kind === "host" ? hostFrame(target.id) : profileFrame(target.id)]);
  const top = stack[stack.length - 1];
  if (!top) return null;

  const replaceTop = (frame: EditorFrame) => setStack((current) => [...current.slice(0, -1), frame]);
  const pop = () => setStack((current) => current.slice(0, -1));

  if (top.kind === "host") {
    return (
      <HostEditor
        frame={top}
        onChange={replaceTop}
        onEditProfile={(profileId) => setStack((current) => [...current, profileFrame(profileId)])}
      />
    );
  }

  const fromHost = stack[stack.length - 2]?.kind === "host";
  return (
    <ProfileEditor
      key={top.draft.id}
      frame={top}
      onChange={replaceTop}
      onBack={fromHost ? pop : undefined}
      onSaved={(profile) => {
        if (!fromHost) {
          useUiStore.getState().closeModal();
          return;
        }
        setStack((current) => {
          const host = current[current.length - 2];
          if (host?.kind !== "host") return current;
          const draft = { ...host.draft, authSource: "profile" as const, profileId: profile.id };
          return [...current.slice(0, -2), { ...host, draft }];
        });
      }}
    />
  );
}
```

The editor keeps its navigation stack in component state, so a different editor target must remount it. Replace the modal layer (only the `key` on `EditorModal` is new):

**`ui/src/shell/ModalLayer.tsx`**
```tsx
import { ConnectScreen } from "../screens/connect/ConnectScreen";
import { EditorModal } from "../screens/editor/EditorModal";
import { ExportDialog } from "../screens/settings/ExportDialog";
import { FirstRunDialog } from "../screens/settings/FirstRunDialog";
import { ImportDialog } from "../screens/settings/ImportDialog";
import { SettingsDialog } from "../screens/settings/SettingsDialog";
import { useUiStore } from "../stores/ui";

/** Renders the single open modal (`useUiStore().modal`). */
export function ModalLayer() {
  const modal = useUiStore((state) => state.modal);
  if (!modal) return null;
  switch (modal.type) {
    case "editor":
      return <EditorModal key={`${modal.target.kind}:${modal.target.id ?? "new"}`} target={modal.target} />;
    case "settings":
      return <SettingsDialog />;
    case "firstRun":
      return <FirstRunDialog />;
    case "export":
      return <ExportDialog />;
    case "import":
      return <ImportDialog />;
    case "connect":
      return <ConnectScreen />;
  }
}
```

**`ui/src/screens/editor/Editor.module.css`**
```css
.headIcon {
  display: grid;
  flex: 0 0 44px;
  place-items: center;
  height: 44px;
  border-radius: 12px;
  background: var(--raised);
  font-family: var(--font-mono);
  font-size: 14px;
  font-weight: 500;
}

.profileIcon {
  color: var(--amber);
}

/* Cancels the dialog body padding: vertical tabs touch the dialog edges. */
.body {
  display: flex;
  flex-direction: column;
  min-height: 520px;
  margin: -22px;
}

.layout {
  display: flex;
  flex: 1;
  min-height: 0;
}

.tabs {
  flex: 0 0 190px;
  padding: 14px 10px;
  border-right: 1px solid var(--bd);
  background: var(--s1b);
}

.panel {
  flex: 1;
  min-width: 0;
  padding: 22px 26px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  align-items: start;
  gap: 16px;
}

.wide {
  grid-column: 1 / -1;
}

.stack {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
}

.banner {
  padding: 10px 12px;
  border: 1px solid var(--accent-bd);
  border-radius: 8px;
  background: var(--accent-soft);
  color: var(--text2);
  font-size: 13px;
}

.info {
  color: var(--text3);
  font-size: 13px;
}

.fieldset {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
}

.legend {
  padding: 0;
  color: var(--text3);
  font-size: 12px;
}

.inline {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 8px;
}

.inline > button {
  margin-top: 24px;
}

.summary {
  display: grid;
  grid-template-columns: 120px minmax(0, 1fr);
  gap: 8px 12px;
  margin: 0;
  padding: 14px 16px;
  border: 1px solid var(--bd);
  border-radius: 10px;
  background: var(--s1);
  font-size: 13px;
}

.summary dt {
  color: var(--muted);
}

.summary dd {
  margin: 0;
}

.swatches {
  display: flex;
  gap: 10px;
}

.swatch {
  width: 28px;
  height: 28px;
  padding: 0;
  border: 2px solid transparent;
  border-radius: 50%;
  box-shadow: 0 0 0 2px var(--dlg);
  cursor: pointer;
}

.swatchChecked {
  border-color: var(--text);
}

.sectionLabel {
  color: var(--muted);
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.hintText {
  margin-top: 4px;
  color: var(--muted);
  font-size: 12px;
}

.errorText {
  color: var(--red);
  font-size: 12px;
}

.forwardList {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.forwardRow {
  display: grid;
  grid-template-columns: 150px minmax(0, 1fr) minmax(0, 1fr) auto auto;
  align-items: end;
  gap: 8px;
}

.addButton {
  height: 40px;
  border: 1px dashed var(--bd-dash);
  border-radius: 10px;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
}

.addButton:hover {
  border-color: var(--bd-hover);
  color: var(--text);
}

.spacer {
  flex: 1;
}

.invalidHint {
  align-self: center;
  color: var(--red);
  font-size: 12px;
}

.status {
  align-self: center;
  color: var(--muted);
  font-size: 13px;
}

.statusOk {
  color: var(--green);
}

.statusFailed {
  color: var(--red);
}

.test {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px 26px;
  border-top: 1px solid var(--bd);
}

.hostKey {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
```

- [ ] **Step 7: Run the tests and the type check**

Run: `bun run test EditorModal AppShell OptionCards TextArea connectProgress steps PromptForm drafts validate`
Expected: PASS (EditorModal 12, AppShell 6, others as in Step 3).

Run: `bun run typecheck`
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add ui/src/components ui/src/stores/connectProgress.ts ui/src/stores/connectProgress.test.ts ui/src/screens/connect ui/src/screens/editor ui/src/shell/AppShell.test.tsx ui/src/shell/ModalLayer.tsx ui/src/i18n/de.ts
git commit -m "feat(ui): add host and login-profile editor with validation, navigation stack and connection test

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 21: Connect flow (store, loading screen, host-key and error screens)

**Files:**
- Create: `ui/src/stores/notices.ts`, `ui/src/stores/streams.ts`, `ui/src/stores/sessionEvents.ts`, `ui/src/stores/terminalSize.ts`
- Modify (full replacement of the shape-only stub): `ui/src/stores/connect.ts`
- Create: `ui/src/shell/useNoticeToasts.ts`
- Modify (full replacement): `ui/src/shell/AppShell.tsx` (drains notices), `ui/src/shell/AppShell.test.tsx`
- Create: `ui/src/screens/connect/connectText.ts`, `ConnectTabBar.tsx`, `ConnectProgress.tsx`, `HostKeyNewDialog.tsx`, `HostKeyChangedDialog.tsx`, `ErrorConnection.tsx`, `ErrorAuth.tsx`, `Connect.module.css`
- Modify (full replacement of the stub): `ui/src/screens/connect/ConnectScreen.tsx`
- Modify: `ui/src/i18n/de.ts` (add `t.connect`)
- Test: `ui/src/stores/connect.test.ts`, `ui/src/screens/connect/connectText.test.ts`, `ui/src/screens/connect/ConnectScreen.test.tsx`

**Interfaces:**
- Consumes: `api.connectSsh/connectAnswer/connectCancel/createEventChannel/createOutputStream/sessionClose/secretSet/newId`; Task 20 `applyConnectEvent`, `asCoreError`, `fireAndForget`, `progressRatio`, `stepItems`, `PromptForm`, `isInlinePrompt`; Task 19 `formatEndpoint`, `hostProfile`, `hostAuthConfig`, `methodLabel`, `initials`, `hostColorVar`; stores `useHostsStore`, `useSessionsStore.addTab/updateTab`, `useSettingsStore.settings.terminalFontSize`, `useUiStore.setView/openModal/closeModal`.
- Produces:
  - `stores/connect.ts`: `ConnectProtocol`, `MAX_ATTEMPTS = 3`, `nextRetryDelay(attempt): number | null` (1→2000, 2→4000, 3→8000, else `null`), `interface ConnectFlow extends StepProgress { connectId; hostId; protocol; failed?: { step; error }; prompt?: { promptId; prompt }; attempt; autoRetry; retrying: boolean; cols; rows }`, `useConnectStore: { active; start(hostId, protocol); answer(promptId, answer); cancel(); retry(); retryWithPassword(password, save); setAutoRetry(on) }`.
  - `stores/notices.ts`: `interface Notice { id; tone: Tone; message; details? }`, `useNoticeStore { notices, push, remove }`, `pushNotice(notice)`.
  - `stores/streams.ts`: `registerStream(tabId, stream)`, `getStream(tabId)`, `dropStream(tabId)` (output streams created before the terminal exists).
  - `stores/sessionEvents.ts`: `sessionEventHandler(tabId): (event: SessionEventDto) => void` (exit → tab `closed`, error → tab `error`).
  - `stores/terminalSize.ts`: `estimateTerminalSize(fontSize, width?, height?): { cols; rows }`.
  - `shell/useNoticeToasts.ts`: `useNoticeToasts()` (shows and removes notices).
  - `screens/connect/connectText.ts`: `type ConnectView`, `connectView(flow)`, `algorithmLabel(algorithm)`, `openSshWarning(algorithm)`, `connectionErrorText(error, host): { title; message; causes; detail? }`.
  - `ConnectScreen()` (same export; renders nothing without an active flow).

- [ ] **Step 1: Write the failing store and text tests**

**`ui/src/stores/connect.test.ts`**
```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { makeHost, makeProfile, withUsage } from "../test/fixtures";
import { MAX_ATTEMPTS, nextRetryDelay, useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useNoticeStore } from "./notices";
import { useSessionsStore } from "./sessions";
import { getStream } from "./streams";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

const deploy = makeProfile();
const web = makeHost({ id: "h-web", name: "web-prod-01" });
const viaProfile = makeHost({ id: "h-prof", name: "kunde-shop", auth: { kind: "profile", profileId: deploy.id } });

type Deferred = { resolve(id: string): void; reject(error: unknown): void };
let attempts: Deferred[] = [];
let channels: Array<(event: never) => void> = [];
const stream = { channel: { stream: true }, attach: vi.fn(), bind: vi.fn(), ack: vi.fn() };

/** Event handler of attempt `n` (each attempt creates the events channel first, then the session channel). */
const eventsOf = (n: number) => channels[n * 2] as (event: api.ConnectEvent) => void;
const sessionEventsOf = (n: number) => channels[n * 2 + 1] as (event: api.SessionEventDto) => void;
const flow = () => useConnectStore.getState().active;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  attempts = [];
  channels = [];
  let next = 0;
  useConnectStore.setState(useConnectStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  useHostsStore.setState({ hosts: [web, viaProfile], profiles: [withUsage(deploy, 1)], loaded: true });
  vi.mocked(api.newId).mockImplementation(() => `id-${++next}`);
  vi.mocked(api.createOutputStream).mockReturnValue(stream as unknown as api.OutputStream);
  vi.mocked(api.createEventChannel).mockImplementation((handler) => {
    channels.push(handler as (event: never) => void);
    return { channel: channels.length } as never;
  });
  vi.mocked(api.connectSsh).mockImplementation(
    () =>
      new Promise<string>((resolve, reject) => {
        attempts.push({ resolve, reject });
      }),
  );
  vi.mocked(api.connectAnswer).mockResolvedValue(true);
  vi.mocked(api.connectCancel).mockResolvedValue(undefined);
  vi.mocked(api.sessionClose).mockResolvedValue(undefined);
  vi.mocked(api.secretSet).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("nextRetryDelay", () => {
  it("backs off 2/4/8 s for the first three attempts, then stops", () => {
    expect([1, 2, 3, 4, 0].map(nextRetryDelay)).toEqual([2000, 4000, 8000, null, null]);
    expect(MAX_ATTEMPTS).toBe(3);
  });
});

describe("connect store", () => {
  it("opens the connect overlay on the protocol's view and reduces events", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: { type: "connect" } });
    expect(flow()).toMatchObject({ connectId: "id-1", hostId: "h-web", protocol: "ssh", attempt: 1, autoRetry: false });
    expect(api.connectSsh).toHaveBeenCalledWith("id-1", "h-web", expect.any(Number), expect.any(Number), { channel: 1 }, stream.channel, { channel: 2 });
    eventsOf(0)({ kind: "steps", steps: ["tcp", "hostKey", "auth", "channel", "shell"] });
    eventsOf(0)({ kind: "stepStarted", step: "tcp" });
    eventsOf(0)({ kind: "log", line: "debug1: Connecting to 10.0.1.12 port 22." });
    expect(flow()).toMatchObject({ current: "tcp", log: ["debug1: Connecting to 10.0.1.12 port 22."] });
    const prompt = { kind: "hostKeyUnknown", host: "10.0.1.12", port: 22, info: { algorithm: "ssh-ed25519", sha256: "SHA256:x", md5: "MD5:y" } } as const;
    eventsOf(0)({ kind: "prompt", promptId: 5, prompt });
    expect(flow()?.prompt).toEqual({ promptId: 5, prompt });
    await useConnectStore.getState().answer(5, { kind: "hostKey", decision: "trust" });
    expect(api.connectAnswer).toHaveBeenCalledWith(5, { kind: "hostKey", decision: "trust" });
    expect(flow()?.prompt).toBeUndefined();
  });

  it("adds a terminal tab with the bound stream when connected", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.resolve("s-1");
    await started;
    expect(stream.bind).toHaveBeenCalledWith("s-1");
    expect(getStream("id-2")).toBe(stream);
    expect(useSessionsStore.getState().tabs).toEqual([
      { id: "id-2", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s-1", status: "connected" },
    ]);
    expect(useUiStore.getState()).toMatchObject({ view: "terminal", modal: null });
    expect(flow()).toBeNull();
    sessionEventsOf(0)({ kind: "exit", code: 0 });
    expect(useSessionsStore.getState().tabs[0]?.status).toBe("closed");
  });

  it("shows network errors and retries automatically with backoff (max 3)", async () => {
    vi.useFakeTimers();
    void useConnectStore.getState().start("h-web", "ssh");
    eventsOf(0)({ kind: "steps", steps: ["tcp"] });
    eventsOf(0)({ kind: "stepStarted", step: "tcp" });
    const timeout = { type: "network", kind: "timeout", message: "connect timed out" };
    attempts[0]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(0);
    expect(flow()).toMatchObject({ failed: { step: "tcp", error: timeout }, retrying: false, attempt: 1 });
    useConnectStore.getState().setAutoRetry(true);
    await vi.advanceTimersByTimeAsync(1999);
    expect(api.connectSsh).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
    expect(flow()).toMatchObject({ attempt: 2, retrying: true, failed: { error: timeout }, connectId: "id-3" });
    attempts[1]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(4000);
    expect(api.connectSsh).toHaveBeenCalledTimes(3);
    attempts[2]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(8000);
    expect(api.connectSsh).toHaveBeenCalledTimes(4);
    attempts[3]?.reject(timeout);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(api.connectSsh).toHaveBeenCalledTimes(4);
    expect(flow()).toMatchObject({ attempt: 4, retrying: false });
  });

  it("retries manually, continuing the counter; auth errors are cleared while retrying", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    expect(flow()?.failed?.error.type).toBe("auth");
    void useConnectStore.getState().retry();
    expect(flow()).toMatchObject({ attempt: 2, retrying: false, failed: undefined, steps: [] });
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
  });

  it("cancels a running attempt and ignores its late outcome", async () => {
    const started = useConnectStore.getState().start("h-web", "ssh");
    await useConnectStore.getState().cancel();
    expect(api.connectCancel).toHaveBeenCalledWith("id-1");
    expect(flow()).toBeNull();
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", modal: null });
    attempts[0]?.resolve("s-late");
    await started;
    expect(api.sessionClose).toHaveBeenCalledWith("s-late");
    expect(useSessionsStore.getState().tabs).toEqual([]);
  });

  it("returns to the overview with a notice when the host key was rejected", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "hostKeyRejected", message: "host key rejected" });
    await settle();
    expect(flow()).toBeNull();
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", modal: null });
    expect(useNoticeStore.getState().notices).toMatchObject([
      { tone: "amber", message: "Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert." },
    ]);
  });

  it("closes silently when the user cancelled a prompt", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "cancelled" });
    await settle();
    expect(flow()).toBeNull();
    expect(useNoticeStore.getState().notices).toEqual([]);
  });

  it("answers the next password prompt with a re-entered password", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    void useConnectStore.getState().retryWithPassword("neu", false);
    await settle();
    expect(api.secretSet).not.toHaveBeenCalled();
    eventsOf(1)({ kind: "prompt", promptId: 9, prompt: { kind: "password", user: "deploy", host: "10.0.1.12", retry: false } });
    expect(api.connectAnswer).toHaveBeenCalledWith(9, { kind: "secret", value: "neu", save: false });
    expect(flow()?.prompt).toBeUndefined();
    eventsOf(1)({ kind: "prompt", promptId: 10, prompt: { kind: "password", user: "deploy", host: "10.0.1.12", retry: true } });
    expect(flow()?.prompt?.promptId).toBe(10);
  });

  it("stores a re-entered password in the login profile before retrying", async () => {
    void useConnectStore.getState().start("h-prof", "ssh");
    attempts[0]?.reject({ type: "auth", rejectedMethods: ["password"], message: "denied" });
    await settle();
    void useConnectStore.getState().retryWithPassword("neu", true);
    await settle();
    expect(api.secretSet).toHaveBeenCalledWith({ kind: "profile", id: "p-deploy" }, "password", "neu");
    expect(api.connectSsh).toHaveBeenCalledTimes(2);
  });

  it("ignores events of superseded attempts", async () => {
    void useConnectStore.getState().start("h-web", "ssh");
    attempts[0]?.reject({ type: "network", kind: "refused", message: "refused" });
    await settle();
    void useConnectStore.getState().retry();
    eventsOf(0)({ kind: "steps", steps: ["jumpHost"] });
    expect(flow()?.steps).toEqual([]);
  });

  it("reports unknown hosts", async () => {
    await useConnectStore.getState().start("missing", "ssh");
    expect(flow()).toBeNull();
    expect(useNoticeStore.getState().notices).toMatchObject([{ tone: "red", message: "Der Host wurde nicht gefunden." }]);
  });
});
```

**`ui/src/screens/connect/connectText.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import type { ConnectFlow } from "../../stores/connect";
import { makeHost } from "../../test/fixtures";
import { algorithmLabel, connectionErrorText, connectView, openSshWarning } from "./connectText";

const base: ConnectFlow = {
  connectId: "c",
  hostId: "h",
  protocol: "ssh",
  steps: [],
  done: [],
  log: [],
  attempt: 1,
  autoRetry: false,
  retrying: false,
  cols: 80,
  rows: 24,
};
const info = { algorithm: "ssh-ed25519", sha256: "SHA256:a", md5: "MD5:b" };

describe("connectView", () => {
  it("prioritises host-key prompts, then inline prompts, then errors", () => {
    expect(connectView(base)).toBe("progress");
    expect(connectView({ ...base, prompt: { promptId: 1, prompt: { kind: "hostKeyUnknown", host: "h", port: 22, info } } })).toBe("hostKeyNew");
    expect(
      connectView({ ...base, prompt: { promptId: 1, prompt: { kind: "hostKeyChanged", host: "h", port: 22, stored: info, current: info } } }),
    ).toBe("hostKeyChanged");
    const failedNetwork = { ...base, failed: { step: "tcp" as const, error: { type: "network" as const, kind: "dns" as const, message: "" } } };
    expect(connectView(failedNetwork)).toBe("errorConnection");
    expect(connectView({ ...failedNetwork, prompt: { promptId: 2, prompt: { kind: "password", user: "u", host: "h", retry: false } } })).toBe("progress");
    expect(connectView({ ...base, failed: { step: "auth", error: { type: "auth", rejectedMethods: [], message: "" } } })).toBe("errorAuth");
    expect(connectView({ ...base, failed: { step: "tcp", error: { type: "config", message: "jump host missing" } } })).toBe("errorConnection");
  });
});

describe("host key texts", () => {
  it("labels algorithms like OpenSSH", () => {
    expect(algorithmLabel("ssh-ed25519")).toBe("ED25519");
    expect(algorithmLabel("ecdsa-sha2-nistp256")).toBe("ECDSA-SHA2-NISTP256");
    expect(algorithmLabel("ssh-rsa")).toBe("RSA");
  });

  it("reproduces the OpenSSH warning banner", () => {
    expect(openSshWarning("ssh-ed25519")).toBe(
      [
        "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@",
        "@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @",
        "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@",
        "Offending ED25519 key in ~/.ssh/known_hosts",
        "Host key verification failed.",
      ].join("\n"),
    );
  });
});

describe("connectionErrorText", () => {
  const host = makeHost({ port: 2222, timeoutS: 20 });

  it("uses the verbatim texts with the host's timeout and port", () => {
    const timeout = connectionErrorText({ type: "network", kind: "timeout", message: "connect timed out" }, host);
    expect(timeout).toEqual({
      title: "Zeitüberschreitung",
      message: "Der Server hat innerhalb von 20 Sekunden nicht geantwortet. Er ist ausgeschaltet, nicht im Netz oder eine Firewall verwirft die Pakete.",
      causes: ["Gerät ist aus oder im Ruhezustand", "Du bist nicht im selben Netz bzw. VPN ist nicht verbunden", "Firewall blockiert Port 2222"],
      detail: "connect timed out",
    });
    expect(connectionErrorText({ type: "network", kind: "refused", message: "x" }, host)).toMatchObject({
      title: "Verbindung abgelehnt",
      message: "Der Server ist erreichbar, aber auf Port 2222 lauscht kein SSH-Dienst.",
      causes: ["sshd ist gestoppt", "SSH läuft auf einem anderen Port", "fail2ban hat deine IP gesperrt"],
    });
    expect(connectionErrorText({ type: "network", kind: "dns", message: "x" }, host)).toMatchObject({
      title: "Host nicht gefunden",
      message: "Der Hostname konnte nicht aufgelöst werden.",
      causes: ["Tippfehler in der Adresse", "DNS-Eintrag fehlt oder ist noch nicht propagiert", "Interner Name, aber kein VPN verbunden"],
    });
  });

  it("falls back to a generic text for other errors", () => {
    expect(connectionErrorText({ type: "config", message: "jump host gone" }, host)).toEqual({
      title: "Verbindung fehlgeschlagen",
      message: "Die Verbindung konnte nicht hergestellt werden.",
      causes: [],
      detail: "jump host gone",
    });
    expect(connectionErrorText({ type: "cancelled" }, host).detail).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `bun run test stores/connect.test connectText`
Expected: FAIL — `nextRetryDelay`/`MAX_ATTEMPTS` are not exported by `./connect`; `./notices`, `./streams`, `./connectText` cannot be resolved.

- [ ] **Step 3: Implement the stores and texts**

**`ui/src/stores/notices.ts`**
```ts
import { create } from "zustand";
import type { Tone } from "../components";

export interface Notice {
  id: number;
  tone: Tone;
  message: string;
  details?: string;
}

export interface NoticeState {
  notices: Notice[];
  push(notice: Omit<Notice, "id">): void;
  remove(id: number): void;
}

let nextNoticeId = 1;

/** Messages from stores (which cannot use the toast context); `useNoticeToasts` shows them. */
export const useNoticeStore = create<NoticeState>()((set) => ({
  notices: [],
  push: (notice) => {
    const id = nextNoticeId;
    nextNoticeId += 1;
    set((state) => ({ notices: [...state.notices, { ...notice, id }] }));
  },
  remove: (id) => set((state) => ({ notices: state.notices.filter((notice) => notice.id !== id) })),
}));

export function pushNotice(notice: Omit<Notice, "id">): void {
  useNoticeStore.getState().push(notice);
}
```

**`ui/src/stores/streams.ts`**
```ts
import type { OutputStream } from "../ipc/api";

/**
 * Output streams by tab id. A stream is created before the session exists (connect flow, local
 * terminal) and attached by `TerminalView` once the tab is rendered; chunks are buffered meanwhile.
 */
const streams = new Map<string, OutputStream>();

export function registerStream(tabId: string, stream: OutputStream): void {
  streams.set(tabId, stream);
}

export function getStream(tabId: string): OutputStream | undefined {
  return streams.get(tabId);
}

export function dropStream(tabId: string): void {
  streams.delete(tabId);
}
```

**`ui/src/stores/sessionEvents.ts`**
```ts
import type { SessionEventDto } from "../ipc/api";
import { useSessionsStore } from "./sessions";

/** Handler for a session's `session_events` channel: the tab shows "Sitzung beendet" afterwards. */
export function sessionEventHandler(tabId: string): (event: SessionEventDto) => void {
  return (event) => {
    useSessionsStore.getState().updateTab(tabId, { status: event.kind === "exit" ? "closed" : "error" });
  };
}
```

**`ui/src/stores/terminalSize.ts`**
```ts
const RAIL_PX = 64;
const TAB_BAR_PX = 48;
const PADDING_PX = 16;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/**
 * PTY size requested before the terminal exists (connect pipeline / local terminal).
 * `TerminalView` fits the real size right after it attaches and sends a resize.
 */
export function estimateTerminalSize(
  fontSize: number,
  width: number = window.innerWidth,
  height: number = window.innerHeight,
): { cols: number; rows: number } {
  const cellWidth = fontSize * 0.6;
  const cellHeight = Math.ceil(fontSize * 1.2);
  return {
    cols: clamp(Math.floor((width - RAIL_PX - PADDING_PX) / cellWidth), 20, 500),
    rows: clamp(Math.floor((height - TAB_BAR_PX - PADDING_PX) / cellHeight), 5, 300),
  };
}
```

**`ui/src/stores/connect.ts`**
```ts
import { create } from "zustand";
import { t } from "../i18n/de";
import {
  type ConnectEvent,
  type CoreError,
  connectAnswer,
  connectCancel,
  connectSsh,
  createEventChannel,
  createOutputStream,
  type Id,
  newId,
  type OutputStream,
  type Prompt,
  type PromptAnswer,
  type SecretOwner,
  type SessionEventDto,
  secretSet,
  sessionClose,
  type Step,
} from "../ipc/api";
import { applyConnectEvent, asCoreError, fireAndForget, type StepProgress } from "./connectProgress";
import { useHostsStore } from "./hosts";
import { pushNotice } from "./notices";
import { sessionEventHandler } from "./sessionEvents";
import { useSessionsStore } from "./sessions";
import { useSettingsStore } from "./settings";
import { registerStream } from "./streams";
import { estimateTerminalSize } from "./terminalSize";
import { useUiStore, type View } from "./ui";

export type ConnectProtocol = "ssh" | "sftp";

/** "Versuch n von 3"; also the number of automatic retries. */
export const MAX_ATTEMPTS = 3;

/** Auto-retry backoff after failed attempt `attempt`: 2 s, 4 s, 8 s, then stop (`null`). */
export function nextRetryDelay(attempt: number): number | null {
  return attempt >= 1 && attempt <= MAX_ATTEMPTS ? 1000 * 2 ** attempt : null;
}

export interface ConnectFlow extends StepProgress {
  connectId: string;
  hostId: Id;
  protocol: ConnectProtocol;
  failed?: { step: Step; error: CoreError };
  prompt?: { promptId: number; prompt: Prompt };
  /** 1-based number of the current/last attempt; retries continue the counter. */
  attempt: number;
  autoRetry: boolean;
  /** A retry after a network error is running; the error screen stays visible in its busy state. */
  retrying: boolean;
  /** PTY size requested for the shell. */
  cols: number;
  rows: number;
}

export interface ConnectState {
  active: ConnectFlow | null;
  start(hostId: Id, protocol: ConnectProtocol): Promise<void>;
  answer(promptId: number, answer: PromptAnswer): Promise<void>;
  cancel(): Promise<void>;
  retry(): Promise<void>;
  /** ErrorAuth: retry with a new password; `save` stores it for the host (or its login profile) first. */
  retryWithPassword(password: string, save: boolean): Promise<void>;
  setAutoRetry(on: boolean): void;
}

let retryTimer: ReturnType<typeof setTimeout> | null = null;
/** Re-entered password for the next password prompt (kept out of the store state on purpose). */
let pendingPassword: string | null = null;

function clearRetryTimer(): void {
  if (retryTimer !== null) clearTimeout(retryTimer);
  retryTimer = null;
}

function secretOwnerOf(hostId: Id): SecretOwner {
  const host = useHostsStore.getState().hosts.find((h) => h.id === hostId);
  return host?.auth.kind === "profile" ? { kind: "profile", id: host.auth.profileId } : { kind: "host", id: hostId };
}

export const useConnectStore = create<ConnectState>()((set, get) => {
  const isCurrent = (connectId: string) => get().active?.connectId === connectId;

  const update = (connectId: string, change: (flow: ConnectFlow) => ConnectFlow) =>
    set((state) => (state.active?.connectId === connectId ? { active: change(state.active) } : state));

  const close = (view: View) => {
    clearRetryTimer();
    pendingPassword = null;
    set({ active: null });
    const ui = useUiStore.getState();
    if (ui.modal?.type === "connect") ui.closeModal();
    ui.setView(view);
  };

  const onEvent = (connectId: string, event: ConnectEvent) => {
    if (!isCurrent(connectId)) return;
    if (event.kind === "prompt") {
      if (event.prompt.kind === "password" && pendingPassword !== null) {
        const value = pendingPassword;
        pendingPassword = null;
        fireAndForget(() => connectAnswer(event.promptId, { kind: "secret", value, save: false }));
        return;
      }
      update(connectId, (flow) => ({ ...flow, prompt: { promptId: event.promptId, prompt: event.prompt } }));
      return;
    }
    update(connectId, (flow) => applyConnectEvent(flow, event));
  };

  const scheduleAutoRetry = () => {
    clearRetryTimer();
    const flow = get().active;
    if (!flow || !flow.autoRetry || flow.retrying || flow.failed?.error.type !== "network") return;
    const delay = nextRetryDelay(flow.attempt);
    if (delay === null) return;
    const { connectId } = flow;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      if (isCurrent(connectId)) void get().retry();
    }, delay);
  };

  const onFailure = (connectId: string, error: CoreError) => {
    if (!isCurrent(connectId)) return;
    pendingPassword = null;
    if (error.type === "cancelled") {
      close("hosts");
      return;
    }
    if (error.type === "hostKeyRejected") {
      close("hosts");
      pushNotice({ tone: "amber", message: t.connect.hostKeyRejected });
      return;
    }
    update(connectId, (flow) => ({
      ...flow,
      failed: { step: flow.failedStep ?? flow.current ?? "tcp", error },
      current: undefined,
      prompt: undefined,
      retrying: false,
    }));
    scheduleAutoRetry();
  };

  const onConnected = (flow: ConnectFlow, sessionId: string, tabId: string, stream: OutputStream) => {
    stream.bind(sessionId);
    registerStream(tabId, stream);
    const title = useHostsStore.getState().hosts.find((h) => h.id === flow.hostId)?.name ?? flow.hostId;
    useSessionsStore.getState().addTab({ id: tabId, title, kind: "ssh", hostId: flow.hostId, sessionId, status: "connected" });
    close("terminal");
  };

  const run = async (flow: ConnectFlow) => {
    const { connectId } = flow;
    const tabId = newId();
    const stream = createOutputStream();
    const events = createEventChannel<ConnectEvent>((event) => onEvent(connectId, event));
    const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
    try {
      const sessionId = await connectSsh(connectId, flow.hostId, flow.cols, flow.rows, events, stream.channel, sessionEvents);
      if (!isCurrent(connectId)) {
        fireAndForget(() => sessionClose(sessionId));
        return;
      }
      onConnected(flow, sessionId, tabId, stream);
    } catch (error) {
      onFailure(connectId, asCoreError(error));
    }
  };

  return {
    active: null,

    start: async (hostId, protocol) => {
      if (!useHostsStore.getState().hosts.some((h) => h.id === hostId)) {
        pushNotice({ tone: "red", message: t.connect.hostMissing });
        return;
      }
      clearRetryTimer();
      pendingPassword = null;
      const previous = get().active;
      if (previous && (!previous.failed || previous.retrying)) fireAndForget(() => connectCancel(previous.connectId));
      const { cols, rows } = estimateTerminalSize(useSettingsStore.getState().settings.terminalFontSize);
      const flow: ConnectFlow = {
        connectId: newId(),
        hostId,
        protocol,
        steps: [],
        done: [],
        log: [],
        attempt: 1,
        autoRetry: false,
        retrying: false,
        cols,
        rows,
      };
      set({ active: flow });
      const ui = useUiStore.getState();
      ui.setView(protocol === "ssh" ? "terminal" : "sftp");
      ui.openModal({ type: "connect" });
      await run(flow);
    },

    answer: async (promptId, answer) => {
      set((state) =>
        state.active?.prompt?.promptId === promptId ? { active: { ...state.active, prompt: undefined } } : state,
      );
      try {
        await connectAnswer(promptId, answer);
      } catch {
        // the prompt timed out or the attempt ended meanwhile
      }
    },

    cancel: async () => {
      const flow = get().active;
      if (!flow) return;
      close("hosts");
      if (!flow.failed || flow.retrying) {
        try {
          await connectCancel(flow.connectId);
        } catch {
          // the attempt already finished
        }
      }
    },

    retry: async () => {
      clearRetryTimer();
      const flow = get().active;
      if (!flow) return;
      const keepError = flow.failed?.error.type === "network";
      const next: ConnectFlow = {
        ...flow,
        connectId: newId(),
        steps: [],
        done: [],
        log: [],
        current: undefined,
        failedStep: undefined,
        prompt: undefined,
        attempt: flow.attempt + 1,
        retrying: keepError,
        failed: keepError ? flow.failed : undefined,
      };
      set({ active: next });
      await run(next);
    },

    retryWithPassword: async (password, save) => {
      const flow = get().active;
      if (!flow) return;
      pendingPassword = password;
      if (save) {
        try {
          await secretSet(secretOwnerOf(flow.hostId), "password", password);
          pendingPassword = null;
        } catch {
          pushNotice({ tone: "amber", message: t.connect.secretFailed });
        }
      }
      await get().retry();
    },

    setAutoRetry: (on) => {
      const flow = get().active;
      if (!flow) return;
      set({ active: { ...flow, autoRetry: on } });
      if (on) scheduleAutoRetry();
      else clearRetryTimer();
    },
  };
});
```

**`ui/src/screens/connect/connectText.ts`**
```ts
import { t } from "../../i18n/de";
import type { CoreError, Host } from "../../ipc/api";
import type { ConnectFlow } from "../../stores/connect";

export type ConnectView = "progress" | "hostKeyNew" | "hostKeyChanged" | "errorConnection" | "errorAuth";

/** Which screen the connect overlay shows: host-key prompts first, inline prompts keep the progress, then errors. */
export function connectView(flow: ConnectFlow): ConnectView {
  const prompt = flow.prompt?.prompt;
  if (prompt?.kind === "hostKeyUnknown") return "hostKeyNew";
  if (prompt?.kind === "hostKeyChanged") return "hostKeyChanged";
  if (prompt) return "progress";
  if (flow.failed) return flow.failed.error.type === "auth" ? "errorAuth" : "errorConnection";
  return "progress";
}

/** `ssh-ed25519` → `ED25519` (OpenSSH style). */
export function algorithmLabel(algorithm: string): string {
  return algorithm.replace(/^ssh-/, "").toUpperCase();
}

/** The classic OpenSSH banner shown under "Technische Details". */
export function openSshWarning(algorithm: string): string {
  return [
    "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@",
    "@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @",
    "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@",
    `Offending ${algorithmLabel(algorithm)} key in ~/.ssh/known_hosts`,
    "Host key verification failed.",
  ].join("\n");
}

export interface ConnectionErrorText {
  title: string;
  message: string;
  causes: string[];
  /** Original backend message (client-style one-liner). */
  detail?: string;
}

export function connectionErrorText(error: CoreError, host: Host): ConnectionErrorText {
  const texts = t.connect.error;
  const detail = "message" in error && error.message ? error.message : undefined;
  if (error.type === "network") {
    switch (error.kind) {
      case "timeout":
        return { title: texts.timeout.title, message: texts.timeout.message(host.timeoutS), causes: [...texts.timeout.causes(host.port)], detail };
      case "refused":
        return { title: texts.refused.title, message: texts.refused.message(host.port), causes: [...texts.refused.causes], detail };
      case "dns":
        return { title: texts.dns.title, message: texts.dns.message, causes: [...texts.dns.causes], detail };
      case "other":
        break;
    }
  }
  return { title: texts.other.title, message: texts.other.message, causes: [], detail };
}
```

**`ui/src/i18n/de.ts`** (insert before `} as const;`)
```ts
  connect: {
    label: (name: string) => `Verbindung zu ${name}`,
    sessions: "Sitzungen",
    connecting: (name: string) => `Verbinde mit ${name} …`,
    via: (endpoint: string, jump: string) => `${endpoint} · über ${jump}`,
    progress: "Verbindungsfortschritt",
    authProfile: (name: string, method: string) => `Profil ${name} · ${method}`,
    shellMeta: (cols: number, rows: number) => `xterm-256color · ${cols}×${rows}`,
    showLog: "▸ Details anzeigen",
    hideLog: "▾ Details ausblenden",
    emptyLog: "Noch keine Ausgabe.",
    tabStatus: {
      waiting: "wartet auf Bestätigung",
      blocked: "blockiert",
      disconnected: "getrennt",
      notLoggedIn: "nicht angemeldet",
    },
    hostMissing: "Der Host wurde nicht gefunden.",
    hostKeyRejected: "Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert.",
    secretFailed: "Speichern im Schlüsselbund derzeit nicht möglich – das Passwort gilt nur für diesen Versuch.",
    attempt: (n: number, max: number) => `Versuch ${n} von ${max}`,
    hostKeyNew: {
      title: "Unbekannter Host",
      textBefore: "Du verbindest dich zum ersten Mal mit ",
      textAfter: ". Vergleiche den Fingerprint mit dem Wert, den dir der Server-Betreiber auf einem anderen Weg genannt hat.",
      caption: (algorithm: string, format: string) => `${algorithm} · ${format}`,
      format: "Fingerprint-Format",
      once: "Nur dieses Mal",
      trust: "Vertrauen und verbinden",
    },
    hostKeyChanged: {
      title: "Host-Schlüssel hat sich geändert",
      textBefore: "Der Server ",
      textAfter:
        " meldet einen anderen Schlüssel als gespeichert. Das passiert nach einer Neuinstallation, kann aber auch ein Angriff (Man-in-the-Middle) sein. Die Verbindung wurde deshalb gestoppt.",
      stored: (algorithm: string) => `Gespeichert · ${algorithm}`,
      current: (algorithm: string) => `Aktuell · ${algorithm}`,
      technical: "Technische Details",
      confirm: "Ich habe den neuen Fingerprint mit dem Betreiber abgeglichen und möchte den gespeicherten Schlüssel ersetzen.",
      replace: "Schlüssel ersetzen und verbinden",
      abort: "Verbindung abbrechen",
    },
    error: {
      retrying: "Neuer Versuch …",
      connecting: "Verbinde …",
      causes: "Mögliche Ursachen",
      toOverview: "Zur Übersicht",
      autoRetry: "Automatisch erneut versuchen",
      timeout: {
        title: "Zeitüberschreitung",
        message: (seconds: number) =>
          `Der Server hat innerhalb von ${seconds} Sekunden nicht geantwortet. Er ist ausgeschaltet, nicht im Netz oder eine Firewall verwirft die Pakete.`,
        causes: (port: number) => [
          "Gerät ist aus oder im Ruhezustand",
          "Du bist nicht im selben Netz bzw. VPN ist nicht verbunden",
          `Firewall blockiert Port ${port}`,
        ],
      },
      refused: {
        title: "Verbindung abgelehnt",
        message: (port: number) => `Der Server ist erreichbar, aber auf Port ${port} lauscht kein SSH-Dienst.`,
        causes: ["sshd ist gestoppt", "SSH läuft auf einem anderen Port", "fail2ban hat deine IP gesperrt"],
      },
      dns: {
        title: "Host nicht gefunden",
        message: "Der Hostname konnte nicht aufgelöst werden.",
        causes: [
          "Tippfehler in der Adresse",
          "DNS-Eintrag fehlt oder ist noch nicht propagiert",
          "Interner Name, aber kein VPN verbunden",
        ],
      },
      other: {
        title: "Verbindung fehlgeschlagen",
        message: "Die Verbindung konnte nicht hergestellt werden.",
      },
    },
    auth: {
      title: "Anmeldung fehlgeschlagen",
      textBefore: "Der Server ",
      rejectedProfile: (name: string) => ` hat die Zugangsdaten aus dem Profil »${name}« abgelehnt.`,
      rejectedHost: " hat die Zugangsdaten dieses Hosts abgelehnt.",
      methods: "Abgelehnte Methoden",
      noMatchingKey: "kein passender Schlüssel",
      rejected: "abgelehnt",
      passwordRejected: (n: number, max: number) => `abgelehnt · Versuch ${n} von ${max}`,
      user: "Benutzer",
      password: "Passwort",
      passwordPlaceholder: "Passwort eingeben",
      passwordRequired: "Bitte ein Passwort eingeben.",
      saveInProfile: (name: string) => `Neues Passwort im Profil »${name}« speichern`,
      saveForHost: "Neues Passwort für diesen Host speichern",
      otherMethod: (method: string) => `Methode: ${method}. Prüfe die Anmeldedaten und versuche es erneut.`,
      editProfile: "Login-Profil bearbeiten",
      retry: "Erneut anmelden",
    },
  },
```

Run: `bun run test stores/connect.test connectText`
Expected: PASS (connect store 12, connectText 5).

- [ ] **Step 4: Write the failing screen tests**

**`ui/src/screens/connect/ConnectScreen.test.tsx`**
```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import { type ConnectFlow, useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { makeHost, makeProfile, withUsage } from "../../test/fixtures";
import { ConnectScreen } from "./ConnectScreen";

vi.mock("../../ipc/api");

const deploy = makeProfile({ name: "deploy", auth: { method: "password", keyPath: null, hasSecret: true } });
const bastion = makeHost({ id: "h-bastion", name: "bastion", address: "bastion.example.net", port: 2222, user: "admin" });
const web = makeHost({ id: "h-web", name: "web-prod-01", jumpHostId: "h-bastion" });
const shop = makeHost({ id: "h-shop", name: "kunde-shop", address: "sftp.kunde-a.example", user: null, auth: { kind: "profile", profileId: deploy.id } });
const keyHost = makeHost({ id: "h-key", name: "nas", auth: { kind: "custom", auth: { method: "key", keyPath: "~/.ssh/id", hasSecret: false } } });
const info = { algorithm: "ssh-ed25519", sha256: "SHA256:Xq7Lm2Pd9vKc", md5: "MD5:3f:9a:c1" };

const actions = {
  cancel: vi.fn(async () => undefined),
  answer: vi.fn(async () => undefined),
  retry: vi.fn(async () => undefined),
  retryWithPassword: vi.fn(async () => undefined),
  setAutoRetry: vi.fn(),
};

function flowFor(hostId: string, patch: Partial<ConnectFlow> = {}): ConnectFlow {
  return {
    connectId: "c1",
    hostId,
    protocol: "ssh",
    steps: [],
    done: [],
    log: [],
    attempt: 1,
    autoRetry: false,
    retrying: false,
    cols: 140,
    rows: 38,
    ...patch,
  };
}

function renderWith(flow: ConnectFlow) {
  useConnectStore.setState({ active: flow, ...actions });
  useUiStore.getState().openModal({ type: "connect" });
  return render(
    <ToastProvider>
      <ConnectScreen />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useConnectStore.setState(useConnectStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState({ hosts: [bastion, web, shop, keyHost], profiles: [withUsage(deploy, 1)], loaded: true });
});

describe("ConnectScreen — progress", () => {
  it("renders nothing without an active flow", () => {
    const { container } = render(<ConnectScreen />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the steps, progress, log and cancel", async () => {
    renderWith(
      flowFor("h-web", {
        steps: ["jumpHost", "tcp", "hostKey", "auth", "channel", "shell"],
        done: ["jumpHost", "tcp"],
        current: "hostKey",
        log: ["debug1: Connecting to bastion.example.net port 2222.", "debug1: Host '10.0.1.12' is known."],
      }),
    );
    const region = screen.getByRole("region", { name: "Verbindung zu web-prod-01" });
    expect(within(region).getByRole("heading", { name: "Verbinde mit web-prod-01 …" })).toBeInTheDocument();
    expect(screen.getByText("deploy@10.0.1.12:22 · über bastion")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "web-prod-01" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("progressbar", { name: "Verbindungsfortschritt" })).toHaveAttribute("aria-valuenow", "33");
    expect(screen.getByText("Jump-Host bastion verbinden")).toBeInTheDocument();
    expect(screen.getByText("admin@bastion.example.net:2222")).toBeInTheDocument();
    expect(screen.queryByText("xterm-256color · 140×38")).not.toBeInTheDocument();
    const toggle = screen.getByRole("button", { name: "▸ Details anzeigen" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(toggle);
    expect(screen.getByRole("button", { name: "▾ Details ausblenden" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(/debug1: Host '10.0.1.12' is known./)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(actions.cancel).toHaveBeenCalledOnce();
  });

  it("asks inline for keyboard-interactive input at the auth step", async () => {
    renderWith(
      flowFor("h-web", {
        steps: ["tcp", "hostKey", "auth"],
        done: ["tcp", "hostKey"],
        current: "auth",
        prompt: { promptId: 4, prompt: { kind: "keyboardInteractive", name: "", instructions: "", prompts: [{ text: "Verification code: ", echo: true }] } },
      }),
    );
    await userEvent.type(screen.getByLabelText("Verification code"), "123456");
    await userEvent.click(screen.getByRole("button", { name: "Senden" }));
    expect(actions.answer).toHaveBeenCalledWith(4, { kind: "responses", values: ["123456"] });
  });

  it("cancels with Escape", async () => {
    renderWith(flowFor("h-web", { steps: ["tcp"], current: "tcp" }));
    screen.getByRole("button", { name: "Abbrechen" }).focus();
    await userEvent.keyboard("{Escape}");
    expect(actions.cancel).toHaveBeenCalledOnce();
  });
});

describe("ConnectScreen — host keys", () => {
  it("asks about an unknown host key", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderWith(flowFor("h-web", { prompt: { promptId: 1, prompt: { kind: "hostKeyUnknown", host: "coolify.example.net", port: 22, info } } }));
    expect(screen.getByRole("tab", { name: "web-prod-01 · wartet auf Bestätigung" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Unbekannter Host" })).toBeInTheDocument();
    expect(screen.getByText(/Du verbindest dich zum ersten Mal mit/)).toHaveTextContent(
      "Du verbindest dich zum ersten Mal mit coolify.example.net:22. Vergleiche den Fingerprint",
    );
    expect(screen.getByText("ED25519 · SHA256")).toBeInTheDocument();
    expect(screen.getByText("SHA256:Xq7Lm2Pd9vKc")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Kopieren" }));
    expect(writeText).toHaveBeenCalledWith("SHA256:Xq7Lm2Pd9vKc");
    expect(await screen.findByRole("button", { name: "Kopiert" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: "MD5" }));
    expect(screen.getByText("MD5:3f:9a:c1")).toBeInTheDocument();
    expect(screen.getByText("ED25519 · MD5")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Nur dieses Mal" }));
    expect(actions.answer).toHaveBeenLastCalledWith(1, { kind: "hostKey", decision: "acceptOnce" });
    await userEvent.click(screen.getByRole("button", { name: "Vertrauen und verbinden" }));
    expect(actions.answer).toHaveBeenLastCalledWith(1, { kind: "hostKey", decision: "trust" });
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(actions.answer).toHaveBeenLastCalledWith(1, { kind: "hostKey", decision: "reject" });
  });

  it("blocks a changed host key until the replacement is confirmed", async () => {
    const current = { algorithm: "ssh-ed25519", sha256: "SHA256:NEW", md5: "MD5:new" };
    renderWith(
      flowFor("h-web", { prompt: { promptId: 2, prompt: { kind: "hostKeyChanged", host: "staging.example.net", port: 22, stored: info, current } } }),
    );
    expect(screen.getByRole("tab", { name: "web-prod-01 · blockiert" })).toBeInTheDocument();
    const alert = screen.getByRole("alertdialog", { name: "Host-Schlüssel hat sich geändert" });
    expect(alert).toHaveTextContent("Der Server staging.example.net meldet einen anderen Schlüssel als gespeichert.");
    expect(within(alert).getByText("Gespeichert · ED25519")).toBeInTheDocument();
    expect(within(alert).getByText("Aktuell · ED25519")).toBeInTheDocument();
    expect(within(alert).getByText("SHA256:NEW")).toBeInTheDocument();
    expect(within(alert).getByText(/WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!/)).toBeInTheDocument();
    const abort = screen.getByRole("button", { name: "Verbindung abbrechen" });
    expect(abort).toHaveAttribute("data-variant", "inverted");
    expect(abort).toHaveFocus();
    const replace = screen.getByRole("button", { name: "Schlüssel ersetzen und verbinden" });
    expect(replace).toBeDisabled();
    await userEvent.click(screen.getByRole("checkbox", { name: /Ich habe den neuen Fingerprint/ }));
    expect(replace).toBeEnabled();
    await userEvent.click(replace);
    expect(actions.answer).toHaveBeenLastCalledWith(2, { kind: "hostKey", decision: "replace" });
    await userEvent.click(abort);
    expect(actions.answer).toHaveBeenLastCalledWith(2, { kind: "hostKey", decision: "reject" });
  });

  it("aborts and opens the host editor", async () => {
    renderWith(
      flowFor("h-web", { prompt: { promptId: 2, prompt: { kind: "hostKeyChanged", host: "h", port: 22, stored: info, current: info } } }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Host bearbeiten" }));
    expect(actions.cancel).toHaveBeenCalledOnce();
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host", id: "h-web" } });
  });
});

describe("ConnectScreen — connection errors", () => {
  it.each([
    ["timeout", "Zeitüberschreitung", "Der Server hat innerhalb von 15 Sekunden nicht geantwortet.", "Firewall blockiert Port 22"],
    ["refused", "Verbindung abgelehnt", "Der Server ist erreichbar, aber auf Port 22 lauscht kein SSH-Dienst.", "fail2ban hat deine IP gesperrt"],
    ["dns", "Host nicht gefunden", "Der Hostname konnte nicht aufgelöst werden.", "Interner Name, aber kein VPN verbunden"],
  ] as const)("shows the %s variant", (kind, title, message, cause) => {
    renderWith(flowFor("h-web", { failed: { step: "tcp", error: { type: "network", kind, message: `ssh: ${kind} details` } } }));
    expect(screen.getByRole("tab", { name: "web-prod-01 · getrennt" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: title })).toBeInTheDocument();
    expect(screen.getByText("deploy@10.0.1.12:22 · Versuch 1 von 3")).toBeInTheDocument();
    expect(screen.getByText(new RegExp(message.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")))).toBeInTheDocument();
    expect(screen.getByText("Mögliche Ursachen")).toBeInTheDocument();
    expect(screen.getByText(cause)).toBeInTheDocument();
    expect(screen.getByText(`ssh: ${kind} details`)).toBeInTheDocument();
  });

  it("retries, toggles auto-retry and leaves to the overview", async () => {
    renderWith(flowFor("h-web", { attempt: 5, failed: { step: "tcp", error: { type: "network", kind: "refused", message: "refused" } } }));
    expect(screen.getByText("deploy@10.0.1.12:22 · Versuch 3 von 3")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Erneut versuchen" }));
    expect(actions.retry).toHaveBeenCalledOnce();
    await userEvent.click(screen.getByRole("checkbox", { name: "Automatisch erneut versuchen" }));
    expect(actions.setAutoRetry).toHaveBeenCalledWith(true);
    await userEvent.click(screen.getByRole("button", { name: "Zur Übersicht" }));
    expect(actions.cancel).toHaveBeenCalledOnce();
  });

  it("shows the busy state while retrying", () => {
    renderWith(
      flowFor("h-web", { attempt: 2, retrying: true, failed: { step: "tcp", error: { type: "network", kind: "timeout", message: "t" } } }),
    );
    expect(screen.getByRole("heading", { name: "Neuer Versuch …" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Verbinde …" })).toBeDisabled();
  });

  it("uses a generic text for non-network errors", () => {
    renderWith(flowFor("h-web", { failed: { step: "tcp", error: { type: "config", message: "jump host h-x of host web not found" } } }));
    expect(screen.getByRole("heading", { name: "Verbindung fehlgeschlagen" })).toBeInTheDocument();
    expect(screen.getByText("jump host h-x of host web not found")).toBeInTheDocument();
  });
});

describe("ConnectScreen — authentication errors", () => {
  const authFailed = (hostId: string) =>
    flowFor(hostId, {
      protocol: "sftp",
      failed: { step: "auth", error: { type: "auth", rejectedMethods: ["publickey", "password"], message: "denied" } },
    });

  it("lists rejected methods and validates the re-entered password", async () => {
    renderWith(authFailed("h-shop"));
    expect(screen.getByRole("tab", { name: "kunde-shop · nicht angemeldet" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Anmeldung fehlgeschlagen" })).toBeInTheDocument();
    expect(screen.getByText(/hat die Zugangsdaten aus dem Profil »deploy« abgelehnt/)).toHaveTextContent(
      "Der Server sftp.kunde-a.example hat die Zugangsdaten aus dem Profil »deploy« abgelehnt.",
    );
    expect(screen.getByText("kein passender Schlüssel")).toBeInTheDocument();
    expect(screen.getByText("abgelehnt · Versuch 1 von 3")).toBeInTheDocument();
    expect(screen.getByLabelText("Benutzer")).toHaveValue("deploy");
    await userEvent.click(screen.getByRole("button", { name: "Erneut anmelden" }));
    expect(screen.getByText("Bitte ein Passwort eingeben.")).toBeInTheDocument();
    expect(screen.getByLabelText("Passwort")).toHaveAttribute("aria-invalid", "true");
    expect(actions.retryWithPassword).not.toHaveBeenCalled();
    await userEvent.type(screen.getByLabelText("Passwort"), "neu");
    expect(screen.getByRole("checkbox", { name: "Neues Passwort im Profil »deploy« speichern" })).toBeChecked();
    await userEvent.click(screen.getByRole("button", { name: "Erneut anmelden" }));
    expect(actions.retryWithPassword).toHaveBeenCalledWith("neu", true);
  });

  it("opens the login profile in the editor", async () => {
    renderWith(authFailed("h-shop"));
    await userEvent.click(screen.getByRole("button", { name: "Login-Profil bearbeiten" }));
    expect(actions.cancel).toHaveBeenCalledOnce();
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "profile", id: "p-deploy" } });
  });

  it("retries without a password form for other methods", async () => {
    renderWith(authFailed("h-key"));
    expect(screen.queryByLabelText("Passwort")).not.toBeInTheDocument();
    expect(screen.getByText(/hat die Zugangsdaten dieses Hosts abgelehnt/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Erneut anmelden" }));
    expect(actions.retry).toHaveBeenCalledOnce();
    await userEvent.click(screen.getByRole("button", { name: "Host bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host", id: "h-key" } });
  });
});
```

- [ ] **Step 5: Run the screen tests to verify they fail**

Run: `bun run test ConnectScreen`
Expected: FAIL — the stub renders a "Verbindung" dialog instead of the overlay (e.g. `Unable to find role="region" and name "Verbindung zu web-prod-01"`).

- [ ] **Step 6: Implement the connect screens, notices toasts and the shell wiring**

**`ui/src/shell/useNoticeToasts.ts`**
```ts
import { useEffect } from "react";
import { useToast } from "../components";
import { useNoticeStore } from "../stores/notices";

/** Shows notices pushed by stores as toasts and removes them from the queue. */
export function useNoticeToasts(): void {
  const notices = useNoticeStore((state) => state.notices);
  const toast = useToast();
  useEffect(() => {
    for (const notice of notices) {
      toast.show({ tone: notice.tone, message: notice.message, details: notice.details });
      useNoticeStore.getState().remove(notice.id);
    }
  }, [notices, toast]);
}
```

**`ui/src/shell/AppShell.tsx`**
```tsx
import { useEffect } from "react";
import { useToast } from "../components";
import { t } from "../i18n/de";
import { errorDetails } from "../ipc/errors";
import { HostsScreen } from "../screens/hosts/HostsScreen";
import { SftpPlaceholder } from "../screens/sftp/SftpPlaceholder";
import { TerminalScreen } from "../screens/terminal/TerminalScreen";
import { useHostsStore } from "../stores/hosts";
import { useSettingsStore } from "../stores/settings";
import { useUiStore } from "../stores/ui";
import styles from "./AppShell.module.css";
import { ModalLayer } from "./ModalLayer";
import { Rail } from "./Rail";
import { useGlobalShortcuts } from "./useGlobalShortcuts";
import { useNoticeToasts } from "./useNoticeToasts";

/**
 * Rail + the three views + modal layer. All views stay mounted (inactive ones are `hidden`)
 * so terminal sessions survive view switches.
 */
export function AppShell() {
  const view = useUiStore((state) => state.view);
  const setTheme = useUiStore((state) => state.setTheme);
  const openModal = useUiStore((state) => state.openModal);
  const theme = useSettingsStore((state) => state.settings.theme);
  const firstRunDone = useSettingsStore((state) => state.settings.firstRunDone);
  const settingsLoaded = useSettingsStore((state) => state.loaded);
  const loadSettings = useSettingsStore((state) => state.load);
  const loadHosts = useHostsStore((state) => state.load);
  const toast = useToast();

  useGlobalShortcuts();
  useNoticeToasts();

  useEffect(() => {
    const report = (error: unknown) =>
      toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) });
    loadSettings().catch(report);
    loadHosts().catch(report);
  }, [loadSettings, loadHosts, toast]);

  useEffect(() => {
    setTheme(theme);
  }, [theme, setTheme]);

  useEffect(() => {
    if (settingsLoaded && !firstRunDone) openModal({ type: "firstRun" });
  }, [settingsLoaded, firstRunDone, openModal]);

  return (
    <div className={styles.shell}>
      <Rail />
      <main className={styles.main}>
        <section className={styles.view} hidden={view !== "hosts"}>
          <HostsScreen />
        </section>
        <section className={styles.view} hidden={view !== "terminal"}>
          <TerminalScreen />
        </section>
        <section className={styles.view} hidden={view !== "sftp"}>
          <SftpPlaceholder />
        </section>
      </main>
      <ModalLayer />
    </div>
  );
}
```

**`ui/src/shell/AppShell.test.tsx`**
```tsx
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../components";
import * as api from "../ipc/api";
import { useHostsStore } from "../stores/hosts";
import { pushNotice, useNoticeStore } from "../stores/notices";
import { DEFAULT_SETTINGS, useSettingsStore } from "../stores/settings";
import { type ModalState, useUiStore } from "../stores/ui";
import { AppShell } from "./AppShell";

vi.mock("../ipc/api");

function mockBackend(settings: Partial<api.Settings> = {}) {
  vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, firstRunDone: true, ...settings });
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
}

function renderShell() {
  return render(
    <ToastProvider>
      <AppShell />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
});

describe("AppShell", () => {
  it("renders the rail and the host overview and loads data", async () => {
    mockBackend();
    renderShell();
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
    await waitFor(() => expect(useSettingsStore.getState().loaded).toBe(true));
    await waitFor(() => expect(useHostsStore.getState().loaded).toBe(true));
  });

  it("switches views via the rail and keeps inactive views mounted", async () => {
    mockBackend();
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Server-Übersicht" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht", hidden: true })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "SFTP" }));
    expect(screen.getByRole("heading", { name: "SFTP" })).toBeInTheDocument();
    expect(screen.getByText("Der SFTP-Dateimanager folgt in einer späteren Version.")).toBeVisible();
  });

  it("applies the persisted theme", async () => {
    mockBackend({ theme: "light" });
    renderShell();
    await waitFor(() => expect(document.documentElement.classList.contains("light")).toBe(true));
  });

  it("opens the first-run dialog while the first run is not done", async () => {
    mockBackend({ firstRunDone: false });
    renderShell();
    expect(await screen.findByRole("dialog", { name: "Willkommen bei LinaSCP" })).toBeInTheDocument();
  });

  it("renders every modal type", async () => {
    mockBackend();
    renderShell();
    const cases: Array<[ModalState, string]> = [
      [{ type: "editor", target: { kind: "host" } }, "Neuer Host"],
      [{ type: "editor", target: { kind: "profile", id: "p1" } }, "Neues Login-Profil"],
      [{ type: "settings" }, "Einstellungen"],
      [{ type: "firstRun" }, "Willkommen bei LinaSCP"],
      [{ type: "export" }, "Exportieren"],
      [{ type: "import" }, "Importieren"],
    ];
    for (const [modal, title] of cases) {
      act(() => useUiStore.getState().openModal(modal));
      expect(screen.getByRole("dialog", { name: title })).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // Without an active connect flow the connect overlay renders nothing.
    act(() => useUiStore.getState().openModal({ type: "connect" }));
    expect(screen.queryByRole("region", { name: /Verbindung zu/ })).not.toBeInTheDocument();
  });

  it("reports load failures as an error toast", async () => {
    mockBackend();
    vi.mocked(api.settingsGet).mockRejectedValue({ type: "store", message: "database is locked" });
    renderShell();
    expect(await screen.findByRole("alert")).toHaveTextContent("Daten konnten nicht geladen werden");
  });

  it("shows store notices as toasts", async () => {
    mockBackend();
    renderShell();
    act(() => pushNotice({ tone: "amber", message: "Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert." }));
    expect(await screen.findByText("Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert.")).toBeInTheDocument();
    expect(useNoticeStore.getState().notices).toEqual([]);
  });
});
```

**`ui/src/screens/connect/ConnectTabBar.tsx`**
```tsx
import { LoaderCircle } from "lucide-react";
import type { Tone } from "../../components";
import { t } from "../../i18n/de";
import styles from "./Connect.module.css";
import type { ConnectView } from "./connectText";

const STATUS: Record<ConnectView, { tone: Tone; suffix?: string }> = {
  progress: { tone: "accent" },
  hostKeyNew: { tone: "amber", suffix: t.connect.tabStatus.waiting },
  hostKeyChanged: { tone: "red", suffix: t.connect.tabStatus.blocked },
  errorConnection: { tone: "red", suffix: t.connect.tabStatus.disconnected },
  errorAuth: { tone: "red", suffix: t.connect.tabStatus.notLoggedIn },
};

/** Single static tab reflecting the connection state ("nas · getrennt"). */
export function ConnectTabBar({ name, view, busy }: { name: string; view: ConnectView; busy: boolean }) {
  const status = STATUS[view];
  const spinning = view === "progress" || busy;
  return (
    <div className={styles.tabBar} role="tablist" aria-label={t.connect.sessions}>
      <span role="tab" aria-selected="true" className={styles.tab} data-tone={spinning ? "accent" : status.tone}>
        {spinning ? (
          <LoaderCircle size={14} className={styles.spin} aria-hidden="true" />
        ) : (
          <span className={styles.dot} aria-hidden="true" />
        )}
        <span>{status.suffix && !busy ? `${name} · ${status.suffix}` : name}</span>
      </span>
    </div>
  );
}
```

**`ui/src/screens/connect/ConnectProgress.tsx`**
```tsx
import { useId, useState } from "react";
import { Button, ProgressBar, StepList } from "../../components";
import { t } from "../../i18n/de";
import type { Host, Step } from "../../ipc/api";
import { formatEndpoint, hostAuthConfig, hostColorVar, hostProfile, initials, methodLabel } from "../../model/hostFormat";
import { type ConnectFlow, useConnectStore } from "../../stores/connect";
import { progressRatio } from "../../stores/connectProgress";
import { useHostsStore } from "../../stores/hosts";
import styles from "./Connect.module.css";
import { isInlinePrompt, PromptForm } from "./PromptForm";
import { stepItems } from "./steps";

export function ConnectProgress({ flow, host }: { flow: ConnectFlow; host: Host }) {
  const hosts = useHostsStore((state) => state.hosts);
  const profiles = useHostsStore((state) => state.profiles);
  const cancel = useConnectStore((state) => state.cancel);
  const answer = useConnectStore((state) => state.answer);
  const [showLog, setShowLog] = useState(false);
  const logId = useId();

  const jump = host.jumpHostId ? hosts.find((h) => h.id === host.jumpHostId) : undefined;
  const profile = hostProfile(host, profiles);
  const auth = hostAuthConfig(host, profiles);
  const endpoint = formatEndpoint(host.user ?? profile?.user, host.address, host.port, true);
  const meta: Partial<Record<Step, string>> = {
    jumpHost: jump ? formatEndpoint(jump.user, jump.address, jump.port, true) : undefined,
    tcp: formatEndpoint(null, host.address, host.port, true),
    auth: profile ? t.connect.authProfile(profile.name, methodLabel(profile.auth.method)) : auth ? methodLabel(auth.method) : undefined,
    channel: "session",
    shell: t.connect.shellMeta(flow.cols, flow.rows),
  };
  const pending = flow.prompt;
  const detail =
    pending && isInlinePrompt(pending.prompt) ? (
      <PromptForm key={pending.promptId} prompt={pending.prompt} onAnswer={(value) => void answer(pending.promptId, value)} />
    ) : null;

  return (
    <div className={styles.column}>
      <div className={styles.header}>
        <span className={styles.pulse} aria-hidden="true">
          <span className={styles.avatar} style={{ color: hostColorVar(host.color) }}>
            {initials(host.name)}
          </span>
        </span>
        <div>
          <h1 className={styles.title}>{t.connect.connecting(host.name)}</h1>
          <p className={styles.subtitle}>{jump ? t.connect.via(endpoint, jump.name) : endpoint}</p>
        </div>
      </div>
      <ProgressBar value={progressRatio(flow)} label={t.connect.progress} />
      <StepList steps={stepItems(flow, { jumpName: jump?.name, meta, detail })} />
      {detail && !flow.current ? detail : null}
      <div>
        <button
          type="button"
          className={styles.logToggle}
          aria-expanded={showLog}
          aria-controls={logId}
          onClick={() => setShowLog((value) => !value)}
        >
          {showLog ? t.connect.hideLog : t.connect.showLog}
        </button>
        {showLog ? (
          <pre id={logId} className={styles.log}>
            {flow.log.length > 0 ? flow.log.join("\n") : t.connect.emptyLog}
          </pre>
        ) : null}
      </div>
      <div className={styles.actions}>
        <Button onClick={() => void cancel()}>{t.common.cancel}</Button>
      </div>
    </div>
  );
}
```

**`ui/src/screens/connect/HostKeyNewDialog.tsx`**
```tsx
import { Copy, ShieldAlert } from "lucide-react";
import { useId, useState } from "react";
import { Button, cx, Segmented } from "../../components";
import { t } from "../../i18n/de";
import type { HostKeyDecision, HostKeyInfo } from "../../ipc/api";
import styles from "./Connect.module.css";
import { algorithmLabel } from "./connectText";

type Format = "sha256" | "md5";

const FORMAT_OPTIONS: ReadonlyArray<{ value: Format; label: string }> = [
  { value: "sha256", label: "SHA256" },
  { value: "md5", label: "MD5" },
];

export function HostKeyNewDialog({
  host,
  port,
  info,
  onDecision,
}: {
  host: string;
  port: number;
  info: HostKeyInfo;
  onDecision: (decision: HostKeyDecision) => void;
}) {
  const [format, setFormat] = useState<Format>("sha256");
  const [copied, setCopied] = useState(false);
  const titleId = useId();
  const fingerprint = format === "sha256" ? info.sha256 : info.md5;
  const texts = t.connect.hostKeyNew;

  const copy = () => {
    navigator.clipboard?.writeText(fingerprint).then(
      () => setCopied(true),
      () => setCopied(false),
    );
  };

  return (
    <section className={styles.card} aria-labelledby={titleId}>
      <span className={cx(styles.icon, styles.iconAmber)} aria-hidden="true">
        <ShieldAlert size={24} />
      </span>
      <h1 id={titleId} className={styles.cardTitle}>
        {texts.title}
      </h1>
      <p className={styles.text}>
        {texts.textBefore}
        <code className={styles.code}>{`${host}:${port}`}</code>
        {texts.textAfter}
      </p>
      <div className={styles.fingerprint}>
        <div className={styles.fingerprintHead}>
          <span>{texts.caption(algorithmLabel(info.algorithm), format === "sha256" ? "SHA256" : "MD5")}</span>
          <Button size="sm" variant="ghost" icon={Copy} onClick={copy}>
            {copied ? t.common.copied : t.common.copy}
          </Button>
        </div>
        <code className={styles.fingerprintValue}>{fingerprint}</code>
        <Segmented
          size="sm"
          label={texts.format}
          value={format}
          options={FORMAT_OPTIONS}
          onChange={(value) => {
            setFormat(value);
            setCopied(false);
          }}
        />
      </div>
      <div className={styles.footer}>
        <Button variant="ghost" onClick={() => onDecision("reject")}>
          {t.common.cancel}
        </Button>
        <span className={styles.spacer} />
        <Button onClick={() => onDecision("acceptOnce")}>{texts.once}</Button>
        <Button variant="primary" onClick={() => onDecision("trust")}>
          {texts.trust}
        </Button>
      </div>
    </section>
  );
}
```

**`ui/src/screens/connect/HostKeyChangedDialog.tsx`**
```tsx
import { TriangleAlert } from "lucide-react";
import { useId, useState } from "react";
import { Button, Checkbox, cx } from "../../components";
import { t } from "../../i18n/de";
import type { HostKeyDecision, HostKeyInfo } from "../../ipc/api";
import { formatEndpoint } from "../../model/hostFormat";
import styles from "./Connect.module.css";
import { algorithmLabel, openSshWarning } from "./connectText";

/**
 * Changed host key: the safe "Verbindung abbrechen" is the visually strongest (inverted) button and
 * focused; "Schlüssel ersetzen und verbinden" stays disabled until the confirmation is ticked.
 */
export function HostKeyChangedDialog({
  host,
  port,
  stored,
  current,
  onDecision,
  onEditHost,
}: {
  host: string;
  port: number;
  stored: HostKeyInfo;
  current: HostKeyInfo;
  onDecision: (decision: HostKeyDecision) => void;
  onEditHost: () => void;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const titleId = useId();
  const textId = useId();
  const texts = t.connect.hostKeyChanged;

  return (
    <section role="alertdialog" aria-labelledby={titleId} aria-describedby={textId} className={cx(styles.card, styles.cardDanger)}>
      <span className={cx(styles.icon, styles.iconRed)} aria-hidden="true">
        <TriangleAlert size={24} />
      </span>
      <h1 id={titleId} className={cx(styles.cardTitle, styles.dangerTitle)}>
        {texts.title}
      </h1>
      <p id={textId} className={styles.text}>
        {texts.textBefore}
        <code className={styles.code}>{formatEndpoint(null, host, port)}</code>
        {texts.textAfter}
      </p>
      <div className={styles.compare}>
        <div className={styles.keyBox}>
          <span className={styles.caption}>{texts.stored(algorithmLabel(stored.algorithm))}</span>
          <code className={styles.keyValue}>{stored.sha256}</code>
        </div>
        <div className={cx(styles.keyBox, styles.keyBoxNew)}>
          <span className={cx(styles.caption, styles.captionRed)}>{texts.current(algorithmLabel(current.algorithm))}</span>
          <code className={styles.keyValue}>{current.sha256}</code>
        </div>
      </div>
      <details className={styles.details}>
        <summary>{texts.technical}</summary>
        <pre className={styles.pre}>{openSshWarning(stored.algorithm)}</pre>
      </details>
      <Checkbox label={texts.confirm} checked={confirmed} onChange={setConfirmed} />
      <div className={styles.footer}>
        <Button variant="danger" disabled={!confirmed} onClick={() => onDecision("replace")}>
          {texts.replace}
        </Button>
        <span className={styles.spacer} />
        <Button variant="ghost" onClick={onEditHost}>
          {t.hosts.edit}
        </Button>
        <Button variant="inverted" autoFocus onClick={() => onDecision("reject")}>
          {texts.abort}
        </Button>
      </div>
    </section>
  );
}
```

**`ui/src/screens/connect/ErrorConnection.tsx`**
```tsx
import { Pencil, RefreshCw, WifiOff } from "lucide-react";
import { useId } from "react";
import { Button, Checkbox, cx } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Host } from "../../ipc/api";
import { formatEndpoint, hostProfile } from "../../model/hostFormat";
import { type ConnectFlow, MAX_ATTEMPTS, useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import styles from "./Connect.module.css";
import { connectionErrorText } from "./connectText";

export function ErrorConnection({
  flow,
  host,
  error,
  onEditHost,
}: {
  flow: ConnectFlow;
  host: Host;
  error: CoreError;
  onEditHost: () => void;
}) {
  const profiles = useHostsStore((state) => state.profiles);
  const retry = useConnectStore((state) => state.retry);
  const cancel = useConnectStore((state) => state.cancel);
  const setAutoRetry = useConnectStore((state) => state.setAutoRetry);
  const titleId = useId();
  const causesId = useId();
  const busy = flow.retrying;
  const text = connectionErrorText(error, host);
  const endpoint = formatEndpoint(host.user ?? hostProfile(host, profiles)?.user, host.address, host.port, true);

  return (
    <section className={styles.page} aria-labelledby={titleId}>
      <span className={cx(styles.icon, styles.iconRed, styles.iconLarge)} aria-hidden="true">
        {busy ? <RefreshCw size={26} className={styles.spin} /> : <WifiOff size={26} />}
      </span>
      <h1 id={titleId} className={styles.cardTitle}>
        {busy ? t.connect.error.retrying : text.title}
      </h1>
      <p className={styles.subtitle}>{`${endpoint} · ${t.connect.attempt(Math.min(flow.attempt, MAX_ATTEMPTS), MAX_ATTEMPTS)}`}</p>
      <p className={styles.text}>{text.message}</p>
      {text.causes.length > 0 ? (
        <div>
          <h2 id={causesId} className={styles.label}>
            {t.connect.error.causes}
          </h2>
          <ul className={styles.causes} aria-labelledby={causesId}>
            {text.causes.map((cause) => (
              <li key={cause}>{cause}</li>
            ))}
          </ul>
        </div>
      ) : null}
      {text.detail ? <pre className={styles.errorPre}>{text.detail}</pre> : null}
      <div className={styles.actions}>
        <Button variant="primary" icon={RefreshCw} disabled={busy} onClick={() => void retry()}>
          {busy ? t.connect.error.connecting : t.common.retry}
        </Button>
        <Button icon={Pencil} onClick={onEditHost}>
          {t.hosts.edit}
        </Button>
        <Button variant="ghost" onClick={() => void cancel()}>
          {t.connect.error.toOverview}
        </Button>
        <span className={styles.spacer} />
        <Checkbox label={t.connect.error.autoRetry} checked={flow.autoRetry} onChange={setAutoRetry} />
      </div>
    </section>
  );
}
```

**`ui/src/screens/connect/ErrorAuth.tsx`**
```tsx
import { Lock } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import { Button, Checkbox, cx, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Host } from "../../ipc/api";
import { hostAuthConfig, hostProfile, methodLabel } from "../../model/hostFormat";
import { type ConnectFlow, MAX_ATTEMPTS, useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import styles from "./Connect.module.css";

function methodStatus(method: string, attempt: number): { text: string; red: boolean } {
  const texts = t.connect.auth;
  if (method === "password") return { text: texts.passwordRejected(Math.min(attempt, MAX_ATTEMPTS), MAX_ATTEMPTS), red: true };
  if (method === "publickey") return { text: texts.noMatchingKey, red: false };
  return { text: texts.rejected, red: true };
}

export function ErrorAuth({
  flow,
  host,
  error,
  onEdit,
}: {
  flow: ConnectFlow;
  host: Host;
  error: CoreError;
  /** Opens the login profile (if used) or the host in the editor. */
  onEdit: (target: { kind: "host" | "profile"; id: string }) => void;
}) {
  const profiles = useHostsStore((state) => state.profiles);
  const retry = useConnectStore((state) => state.retry);
  const retryWithPassword = useConnectStore((state) => state.retryWithPassword);
  const cancel = useConnectStore((state) => state.cancel);
  const [password, setPassword] = useState("");
  const [save, setSave] = useState(true);
  const [missing, setMissing] = useState(false);
  const titleId = useId();
  const formId = useId();
  const texts = t.connect.auth;

  const profile = hostProfile(host, profiles);
  const auth = hostAuthConfig(host, profiles);
  const askPassword = auth?.method === "password";
  const rejected = error.type === "auth" ? error.rejectedMethods : [];

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    if (!askPassword) {
      void retry();
      return;
    }
    if (!password) {
      setMissing(true);
      return;
    }
    void retryWithPassword(password, save);
  };

  return (
    <section className={cx(styles.card, styles.cardNarrow)} aria-labelledby={titleId}>
      <span className={cx(styles.icon, styles.iconRed)} aria-hidden="true">
        <Lock size={24} />
      </span>
      <h1 id={titleId} className={styles.cardTitle}>
        {texts.title}
      </h1>
      <p className={styles.text}>
        {texts.textBefore}
        <code className={styles.code}>{host.address}</code>
        {profile ? texts.rejectedProfile(profile.name) : texts.rejectedHost}
      </p>
      {rejected.length > 0 ? (
        <ul className={styles.methods} aria-label={texts.methods}>
          {rejected.map((method) => {
            const status = methodStatus(method, flow.attempt);
            return (
              <li key={method}>
                <span className="mono">{method}</span>
                <span className={status.red ? styles.methodRed : styles.methodMuted}>{status.text}</span>
              </li>
            );
          })}
        </ul>
      ) : null}
      {askPassword ? (
        <form id={formId} className={styles.form} onSubmit={submit} noValidate>
          <TextField label={texts.user} mono readOnly value={host.user ?? profile?.user ?? ""} onChange={() => undefined} />
          <TextField
            type="password"
            label={texts.password}
            placeholder={texts.passwordPlaceholder}
            autoFocus
            autoComplete="off"
            value={password}
            error={missing && !password ? texts.passwordRequired : null}
            onChange={(value) => {
              setPassword(value);
              setMissing(false);
            }}
          />
          <Checkbox label={profile ? texts.saveInProfile(profile.name) : texts.saveForHost} checked={save} onChange={setSave} />
        </form>
      ) : (
        <p className={styles.text}>{texts.otherMethod(auth ? methodLabel(auth.method) : t.hosts.none)}</p>
      )}
      <div className={styles.footer}>
        <Button variant="ghost" onClick={() => void cancel()}>
          {t.common.cancel}
        </Button>
        <Button onClick={() => onEdit(profile ? { kind: "profile", id: profile.id } : { kind: "host", id: host.id })}>
          {profile ? texts.editProfile : t.hosts.edit}
        </Button>
        <span className={styles.spacer} />
        {askPassword ? (
          <Button variant="primary" type="submit" form={formId}>
            {texts.retry}
          </Button>
        ) : (
          <Button variant="primary" onClick={() => submit()}>
            {texts.retry}
          </Button>
        )}
      </div>
    </section>
  );
}
```

**`ui/src/screens/connect/ConnectScreen.tsx`**
```tsx
import type { KeyboardEvent } from "react";
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
  if (!flow || !host) return null;

  const view = connectView(flow);
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
    <section className={styles.overlay} aria-label={t.connect.label(host.name)} onKeyDown={onKeyDown}>
      <ConnectTabBar name={host.name} view={view} busy={flow.retrying} />
      <div className={styles.content}>{body}</div>
    </section>
  );
}
```

**`ui/src/screens/connect/Connect.module.css`**
```css
.overlay {
  position: fixed;
  inset: 0 0 0 64px;
  z-index: 50;
  display: flex;
  flex-direction: column;
  background: var(--bg);
  color: var(--text);
}

.tabBar {
  display: flex;
  align-items: stretch;
  min-height: 48px;
  padding: 0 8px;
  border-bottom: 1px solid var(--bd);
  background: var(--s1);
}

.tab {
  --tab-tone: var(--accent);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 16px;
  background: var(--bg);
  box-shadow: inset 0 2px 0 var(--tab-tone);
  font-size: 13px;
}

.tab[data-tone="amber"] {
  --tab-tone: var(--amber);
}

.tab[data-tone="red"] {
  --tab-tone: var(--red);
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--tab-tone);
}

.spin {
  color: var(--accent);
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.content {
  display: flex;
  flex: 1;
  align-items: flex-start;
  justify-content: center;
  padding: 40px 16px;
  overflow-y: auto;
}

.column {
  display: flex;
  flex-direction: column;
  gap: 20px;
  width: 100%;
  max-width: 560px;
}

.header {
  display: flex;
  align-items: center;
  gap: 16px;
}

.pulse {
  position: relative;
  display: grid;
  flex: 0 0 60px;
  place-items: center;
  height: 60px;
}

.pulse::before {
  position: absolute;
  inset: 0;
  border-radius: 16px;
  background: var(--accent-soft);
  animation: pulse 1.6s ease-out infinite;
  content: "";
}

@keyframes pulse {
  from {
    opacity: 0.55;
    transform: scale(1);
  }
  to {
    opacity: 0;
    transform: scale(1.7);
  }
}

.avatar {
  position: relative;
  display: grid;
  place-items: center;
  width: 60px;
  height: 60px;
  border-radius: 16px;
  background: var(--raised);
  font-family: var(--font-mono);
  font-size: 18px;
  font-weight: 500;
}

.title {
  font-size: 20px;
  font-weight: 600;
}

.subtitle {
  margin-top: 2px;
  color: var(--muted);
  font-family: var(--font-mono);
  font-size: 13px;
}

.logToggle {
  padding: 4px 0;
  border: 0;
  background: transparent;
  color: var(--muted);
  font-size: 13px;
  cursor: pointer;
}

.logToggle:hover {
  color: var(--text);
}

.log {
  max-height: 180px;
  margin: 8px 0 0;
  padding: 12px;
  overflow: auto;
  border: 1px solid var(--bd);
  border-radius: 8px;
  background: var(--s1);
  color: var(--text2);
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: pre-wrap;
}

.actions,
.footer {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.spacer {
  flex: 1;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 18px;
  width: 100%;
  max-width: 620px;
  padding: 28px;
  border: 1px solid var(--bd-strong);
  border-radius: 16px;
  background: var(--dlg);
}

.cardDanger {
  max-width: 660px;
  border-color: var(--red-bd);
}

.cardNarrow {
  max-width: 560px;
}

.page {
  display: flex;
  flex-direction: column;
  gap: 16px;
  width: 100%;
  max-width: 600px;
}

.icon {
  display: grid;
  place-items: center;
  width: 48px;
  height: 48px;
  border-radius: 12px;
}

.iconLarge {
  width: 52px;
  height: 52px;
}

.iconAmber {
  background: var(--amber-soft);
  color: var(--amber);
}

.iconRed {
  background: var(--red-soft);
  color: var(--red);
}

.iconRed .spin {
  color: var(--red);
}

.cardTitle {
  font-size: 20px;
  font-weight: 600;
}

.dangerTitle {
  color: var(--red-text);
}

.text {
  color: var(--text2);
  line-height: 1.55;
}

.code {
  padding: 1px 6px;
  border-radius: 6px;
  background: var(--raised);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 13px;
}

.fingerprint {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
  padding: 16px;
  border: 1px solid var(--bd);
  border-radius: 12px;
  background: var(--s1);
}

.fingerprintHead {
  display: flex;
  align-items: center;
  justify-content: space-between;
  align-self: stretch;
  color: var(--muted);
  font-size: 12px;
}

.fingerprintValue {
  font-family: var(--font-mono);
  font-size: 14px;
  overflow-wrap: anywhere;
}

.compare {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.keyBox {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 14px;
  border: 1px solid var(--bd);
  border-radius: 12px;
  background: var(--s1);
}

.keyBoxNew {
  border-color: var(--red-bd);
  background: var(--red-bg2);
}

.caption {
  color: var(--muted);
  font-size: 12px;
}

.captionRed {
  color: var(--red);
}

.keyValue {
  font-family: var(--font-mono);
  font-size: 13px;
  overflow-wrap: anywhere;
}

.details summary {
  color: var(--muted);
  font-size: 13px;
  cursor: pointer;
}

.pre {
  margin: 10px 0 0;
  padding: 12px;
  overflow: auto;
  border-radius: 8px;
  background: var(--s1);
  font-family: var(--font-mono);
  font-size: 12px;
}

.label {
  color: var(--muted);
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.causes {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 8px 0 0;
  padding: 0;
  color: var(--text2);
  list-style: none;
}

.causes li::before {
  color: var(--muted);
  content: "– ";
}

.errorPre {
  margin: 0;
  padding: 12px;
  overflow: auto;
  border: 1px solid var(--bd);
  border-radius: 8px;
  background: var(--s1);
  color: var(--red-text);
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: pre-wrap;
}

.methods {
  display: flex;
  flex-direction: column;
  margin: 0;
  padding: 4px 14px;
  border: 1px solid var(--bd);
  border-radius: 10px;
  background: var(--s1);
  font-size: 13px;
  list-style: none;
}

.methods li {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 0;
}

.methods li + li {
  border-top: 1px solid var(--bd);
}

.methodRed {
  color: var(--red);
}

.methodMuted {
  color: var(--muted);
}

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
```

- [ ] **Step 7: Run the tests and the type check**

Run: `bun run test connect ConnectScreen AppShell`
Expected: PASS — 8 files, 58 tests (connect store 12, connectText 5, ConnectScreen 16, AppShell 7; the `connect` filter also matches `connectProgress`, `PromptForm`, `steps` and `quickConnect`).

Run: `bun run typecheck`
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add ui/src/stores ui/src/shell ui/src/screens/connect ui/src/i18n/de.ts
git commit -m "feat(ui): add connect flow with progress, host-key decisions, error screens and auto-retry

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 22: Terminal screen (TabBar, TerminalView, local terminals)

**Files:**
- Modify: `package.json` / `bun.lock` (xterm packages via `bun add`)
- Create: `ui/src/screens/terminal/schemes.json` (copied from the design zip), `theme.ts`, `terminalConfig.ts`, `clipboard.ts`, `TerminalView.tsx` + `.module.css`, `TabBar.tsx` + `.module.css`, `SessionEnded.tsx` + `.module.css`, `TerminalScreen.module.css`
- Modify (full replacement of the stub): `ui/src/screens/terminal/TerminalScreen.tsx`
- Modify (full replacement): `ui/src/stores/sessions.ts` (`openLocal` implemented, new `closeSession`)
- Modify: `ui/src/i18n/de.ts` (add `t.terminal`)
- Test: `ui/src/screens/terminal/terminalConfig.test.ts`, `ui/src/screens/terminal/TerminalView.test.tsx`, `ui/src/screens/terminal/TerminalScreen.test.tsx`, `ui/src/stores/sessions.test.ts`

**Interfaces:**
- Consumes: `api.sessionOpenLocal/sessionWrite/sessionResize/sessionClose/createOutputStream/createEventChannel/newId`; Task 21 `registerStream/getStream/dropStream`, `sessionEventHandler`, `estimateTerminalSize`, `pushNotice`, `useConnectStore.start`; `useSettingsStore.settings.{terminalRenderer, terminalFontSize}`, `useUiStore.{theme, view, setView}`; `fireAndForget`.
- Produces:
  - `stores/sessions.ts`: unchanged `Tab`/`TabKind`/`TabStatus` and actions, plus `openLocal(): Promise<void>` (local PTY tab "Lokales Terminal", "Lokales Terminal 2", …; errors become a red notice) and `closeSession(id): Promise<void>` (removes the tab, drops its stream, `sessionClose` best effort).
  - `screens/terminal/theme.ts`: `TERMINAL_FONT_FAMILY`, `FONT_SIZE_MIN`, `FONT_SIZE_MAX` (from `schemes.json`), `terminalTheme(appTheme: Theme): ITheme` (`linascp` dark / `linascpLight` light = scheme "auto").
  - `screens/terminal/terminalConfig.ts`: `shouldUseWebgl(setting, visible, failed)`, `clipboardShortcut(event): "copy" | "paste" | null`.
  - `screens/terminal/clipboard.ts`: `copyText(text)`, `readText()` (never throw).
  - `TerminalView({ tab, visible })` — container carries `data-terminal`; `TabBar()`, `tabButtonId(id)`, `tabPanelId(id)`; `SessionEnded({ tab })`; `TerminalScreen()` (keeps a visually hidden `h1` "Terminal").

- [ ] **Step 1: Add the xterm packages and the color schemes**

Run:
```bash
bun add @xterm/xterm@^6.0.0 @xterm/addon-fit@^0.11.0 @xterm/addon-webgl@^0.19.0 @xterm/addon-web-links@^0.12.0 @xterm/addon-unicode11@^0.9.0
mkdir -p ui/src/screens/terminal
unzip -p shellport-design.zip shellport-design/tokens/terminal-schemes.json \
  | sed -e 's/Shellport/LinaSCP/g' -e 's/shellport/linascp/g' > ui/src/screens/terminal/schemes.json
grep -c '"linascp' ui/src/screens/terminal/schemes.json
```
Expected: `bun.lock` updated; the grep prints `2` (keys `linascp` and `linascpLight`; "Shellport" was only the working title). No `resolveJsonModule` change is needed (TypeScript 7 with `moduleResolution: "bundler"` types JSON imports).

- [ ] **Step 2: Write the failing tests for the pure helpers and the sessions store**

**`ui/src/screens/terminal/terminalConfig.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { clipboardShortcut, shouldUseWebgl } from "./terminalConfig";
import { FONT_SIZE_MAX, FONT_SIZE_MIN, TERMINAL_FONT_FAMILY, terminalTheme } from "./theme";

const key = (key: string, mods: Partial<KeyboardEvent> = {}) =>
  ({ key, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods }) as KeyboardEvent;

describe("terminal theme", () => {
  it("uses the opaque default scheme of the app theme", () => {
    expect(terminalTheme("dark")).toMatchObject({ background: "#0B0E13", foreground: "#D4DAE2", cursor: "#4FD1C5", green: "#3DDC97" });
    expect(terminalTheme("light")).toMatchObject({ background: "#FFFFFF", foreground: "#2A313B", cursor: "#0E7C72" });
    expect(terminalTheme("dark").background).toMatch(/^#[0-9A-F]{6}$/);
    expect([FONT_SIZE_MIN, FONT_SIZE_MAX]).toEqual([10, 20]);
    expect(TERMINAL_FONT_FAMILY).toContain("JetBrains Mono");
  });
});

describe("shouldUseWebgl", () => {
  it("uses WebGL only on visible terminals that have not failed and are not set to DOM", () => {
    expect(shouldUseWebgl("auto", true, false)).toBe(true);
    expect(shouldUseWebgl("webgl", true, false)).toBe(true);
    expect(shouldUseWebgl("auto", false, false)).toBe(false);
    expect(shouldUseWebgl("auto", true, true)).toBe(false);
    expect(shouldUseWebgl("dom", true, false)).toBe(false);
  });
});

describe("clipboardShortcut", () => {
  it("detects Ctrl+Shift+C/V only", () => {
    expect(clipboardShortcut(key("C", { ctrlKey: true, shiftKey: true }))).toBe("copy");
    expect(clipboardShortcut(key("v", { ctrlKey: true, shiftKey: true }))).toBe("paste");
    expect(clipboardShortcut(key("c", { ctrlKey: true }))).toBeNull();
    expect(clipboardShortcut(key("v", { ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
    expect(clipboardShortcut(key("t", { ctrlKey: true, shiftKey: true }))).toBeNull();
  });
});
```

**`ui/src/stores/sessions.test.ts`**
```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { useNoticeStore } from "./notices";
import { useSessionsStore } from "./sessions";
import { getStream, registerStream } from "./streams";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

const stream = { channel: { raw: true }, attach: vi.fn(), bind: vi.fn(), ack: vi.fn() };
let sessionHandlers: Array<(event: api.SessionEventDto) => void> = [];

beforeEach(() => {
  let next = 0;
  sessionHandlers = [];
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  vi.mocked(api.newId).mockImplementation(() => `t-${++next}`);
  vi.mocked(api.createOutputStream).mockReturnValue(stream as unknown as api.OutputStream);
  vi.mocked(api.createEventChannel).mockImplementation((handler) => {
    sessionHandlers.push(handler as (event: api.SessionEventDto) => void);
    return { events: true } as never;
  });
  vi.mocked(api.sessionClose).mockResolvedValue(undefined);
});

describe("sessions store — local terminals", () => {
  it("opens numbered local terminal tabs and shows the terminal view", async () => {
    vi.mocked(api.sessionOpenLocal).mockResolvedValueOnce("s-1").mockResolvedValueOnce("s-2");
    await useSessionsStore.getState().openLocal();
    expect(api.sessionOpenLocal).toHaveBeenCalledWith(expect.any(Number), expect.any(Number), stream.channel, { events: true });
    expect(stream.bind).toHaveBeenCalledWith("s-1");
    expect(getStream("t-1")).toBe(stream);
    expect(useUiStore.getState().view).toBe("terminal");
    await useSessionsStore.getState().openLocal();
    expect(useSessionsStore.getState().tabs).toEqual([
      { id: "t-1", title: "Lokales Terminal", kind: "local", sessionId: "s-1", status: "connected" },
      { id: "t-2", title: "Lokales Terminal 2", kind: "local", sessionId: "s-2", status: "connected" },
    ]);
    expect(useSessionsStore.getState().activeId).toBe("t-2");
    sessionHandlers[0]?.({ kind: "exit", code: 0 });
    expect(useSessionsStore.getState().tabs[0]?.status).toBe("closed");
  });

  it("reports a failed start as a notice", async () => {
    vi.mocked(api.sessionOpenLocal).mockRejectedValue({ type: "io", message: "no pty" });
    await useSessionsStore.getState().openLocal();
    expect(useSessionsStore.getState().tabs).toEqual([]);
    expect(useNoticeStore.getState().notices).toMatchObject([{ tone: "red", message: "Lokales Terminal konnte nicht gestartet werden" }]);
  });

  it("closes a session: removes the tab and stream and closes the backend session", async () => {
    registerStream("a", stream as unknown as api.OutputStream);
    useSessionsStore.getState().addTab({ id: "a", title: "web", kind: "ssh", hostId: "h", sessionId: "s-a", status: "connected" });
    await useSessionsStore.getState().closeSession("a");
    expect(useSessionsStore.getState().tabs).toEqual([]);
    expect(getStream("a")).toBeUndefined();
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
  });
});
```

- [ ] **Step 3: Run them to verify they fail**

Run: `bun run test terminalConfig sessions.test`
Expected: FAIL — `./terminalConfig` and `./theme` cannot be resolved; `openLocal` creates no tab (stub) and `closeSession` is not a function.

- [ ] **Step 4: Implement the helpers, the theme and the sessions store**

**`ui/src/screens/terminal/theme.ts`**
```ts
import type { ITheme } from "@xterm/xterm";
import type { Theme } from "../../ipc/api";
import schemes from "./schemes.json";

interface Scheme {
  name: string;
  bg: string;
  fg: string;
  dim: string;
  green: string;
  blue: string;
  yellow: string;
  red: string;
  cursor: string;
  bd: string;
}

/** Scheme "auto" (default until TP2 adds the scheme picker): dark → `linascp`, light → `linascpLight`. */
const DARK: Scheme = schemes.schemes.linascp;
const LIGHT: Scheme = schemes.schemes.linascpLight;

export const TERMINAL_FONT_FAMILY = '"JetBrains Mono", monospace';
export const FONT_SIZE_MIN: number = schemes.fontSize.min;
export const FONT_SIZE_MAX: number = schemes.fontSize.max;

/** xterm theme with an opaque background (transparency causes artifacts in WebKitGTK). */
export function terminalTheme(appTheme: Theme): ITheme {
  const s = appTheme === "light" ? LIGHT : DARK;
  return {
    background: s.bg,
    foreground: s.fg,
    cursor: s.cursor,
    cursorAccent: s.bg,
    selectionBackground: `${s.cursor}55`,
    black: s.bd,
    brightBlack: s.dim,
    red: s.red,
    brightRed: s.red,
    green: s.green,
    brightGreen: s.green,
    yellow: s.yellow,
    brightYellow: s.yellow,
    blue: s.blue,
    brightBlue: s.blue,
    white: s.fg,
    brightWhite: s.fg,
  };
}
```

**`ui/src/screens/terminal/terminalConfig.ts`**
```ts
import type { TerminalRenderer } from "../../ipc/api";

/**
 * WebGL only on the visible terminal (WebKit allows ~16 live WebGL contexts) and only until it
 * failed once for this terminal; `dom` never uses WebGL.
 */
export function shouldUseWebgl(setting: TerminalRenderer, visible: boolean, failed: boolean): boolean {
  return visible && setting !== "dom" && !failed;
}

type KeyInfo = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** Ctrl+Shift+C copies the selection, Ctrl+Shift+V pastes (plain Ctrl+C/V belong to the shell). */
export function clipboardShortcut(event: KeyInfo): "copy" | "paste" | null {
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (key === "c") return "copy";
  if (key === "v") return "paste";
  return null;
}
```

**`ui/src/screens/terminal/clipboard.ts`**
```ts
/**
 * Clipboard access for the terminal. WebKitGTK does not expose the X11/Wayland PRIMARY selection to
 * web content, so "select to copy / middle click to paste" uses the regular clipboard.
 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard?.writeText(text);
  } catch {
    // clipboard unavailable (permissions) – selection simply stays local
  }
}

export async function readText(): Promise<string> {
  try {
    return (await navigator.clipboard?.readText()) ?? "";
  } catch {
    return "";
  }
}
```

**`ui/src/stores/sessions.ts`**
```ts
import { create } from "zustand";
import { t } from "../i18n/de";
import {
  createEventChannel,
  createOutputStream,
  type Id,
  newId,
  type SessionEventDto,
  type SessionId,
  sessionClose,
  sessionOpenLocal,
} from "../ipc/api";
import { errorDetails } from "../ipc/errors";
import { pushNotice } from "./notices";
import { sessionEventHandler } from "./sessionEvents";
import { useSettingsStore } from "./settings";
import { dropStream, registerStream } from "./streams";
import { estimateTerminalSize } from "./terminalSize";
import { useUiStore } from "./ui";

export type TabKind = "ssh" | "local";
export type TabStatus = "connecting" | "connected" | "closed" | "error";

export interface Tab {
  id: string;
  title: string;
  kind: TabKind;
  hostId?: Id;
  sessionId?: SessionId;
  status: TabStatus;
}

export interface SessionsState {
  tabs: Tab[];
  activeId: string | null;
  /** Appends and activates the tab. */
  addTab(tab: Tab): void;
  /** Removes the tab; if it was active, activates the right neighbour, else the left one. */
  closeTab(id: string): void;
  setActive(id: string): void;
  updateTab(id: string, patch: Partial<Omit<Tab, "id">>): void;
  /** Opens a local terminal tab (Ctrl+Shift+T, "+" in the tab bar); failures become a notice. */
  openLocal(): Promise<void>;
  /** Closes the tab and its backend session (best effort). */
  closeSession(id: string): Promise<void>;
}

export const useSessionsStore = create<SessionsState>()((set, get) => ({
  tabs: [],
  activeId: null,
  addTab: (tab) => set((state) => ({ tabs: [...state.tabs, tab], activeId: tab.id })),
  closeTab: (id) =>
    set((state) => {
      const index = state.tabs.findIndex((tab) => tab.id === id);
      if (index === -1) return state;
      const tabs = state.tabs.filter((tab) => tab.id !== id);
      if (state.activeId !== id) return { tabs };
      const neighbour = tabs[index] ?? tabs[index - 1];
      return { tabs, activeId: neighbour ? neighbour.id : null };
    }),
  setActive: (activeId) => set({ activeId }),
  updateTab: (id, patch) =>
    set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === id ? { ...tab, ...patch } : tab)) })),
  openLocal: async () => {
    const tabId = newId();
    const stream = createOutputStream();
    const sessionEvents = createEventChannel<SessionEventDto>(sessionEventHandler(tabId));
    const { cols, rows } = estimateTerminalSize(useSettingsStore.getState().settings.terminalFontSize);
    try {
      const sessionId = await sessionOpenLocal(cols, rows, stream.channel, sessionEvents);
      stream.bind(sessionId);
      registerStream(tabId, stream);
      const count = get().tabs.filter((tab) => tab.kind === "local").length;
      const title = count === 0 ? t.terminal.localTitle : `${t.terminal.localTitle} ${count + 1}`;
      get().addTab({ id: tabId, title, kind: "local", sessionId, status: "connected" });
      useUiStore.getState().setView("terminal");
    } catch (error) {
      pushNotice({ tone: "red", message: t.terminal.localFailed, details: errorDetails(error) });
    }
  },
  closeSession: async (id) => {
    const tab = get().tabs.find((candidate) => candidate.id === id);
    get().closeTab(id);
    dropStream(id);
    if (!tab?.sessionId) return;
    try {
      await sessionClose(tab.sessionId);
    } catch {
      // the session already ended
    }
  },
}));
```

**`ui/src/i18n/de.ts`** (insert before `} as const;`)
```ts
  terminal: {
    title: "Terminal",
    sessions: "Sitzungen",
    label: (title: string) => `Terminal ${title}`,
    emptyTitle: "Keine offenen Sitzungen",
    emptyText: "Verbinde dich in der Server-Übersicht mit einem Host oder öffne ein lokales Terminal.",
    openLocal: "Lokales Terminal öffnen",
    newLocal: "Neues lokales Terminal",
    newLocalTitle: "Neues lokales Terminal (Ctrl+Shift+T)",
    localTitle: "Lokales Terminal",
    localFailed: "Lokales Terminal konnte nicht gestartet werden",
    closeTab: (title: string) => `${title} schließen`,
    ended: "Sitzung beendet",
    endedError: "Sitzung mit Fehler beendet",
    endedText: (title: string) => `Die Sitzung »${title}« wurde beendet. Der Verlauf bleibt sichtbar, bis du den Tab schließt.`,
    endedSuffix: "beendet",
    reconnect: "Neu verbinden",
  },
```

Run: `bun run test terminalConfig sessions.test stores.test`
Expected: PASS (terminalConfig 3, sessions 3, stores 10).

- [ ] **Step 5: Write the failing TerminalView and TerminalScreen tests**

**`ui/src/screens/terminal/TerminalView.test.tsx`**
```tsx
import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Tab } from "../../stores/sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { dropStream, registerStream } from "../../stores/streams";
import { useUiStore } from "../../stores/ui";
import { TerminalView } from "./TerminalView";

const fakes = vi.hoisted(() => {
  class Emitter<T> {
    listeners: Array<(value: T) => void> = [];
    event = (listener: (value: T) => void) => {
      this.listeners.push(listener);
      return { dispose: () => (this.listeners = this.listeners.filter((l) => l !== listener)) };
    };
    fire(value: T) {
      for (const listener of this.listeners) listener(value);
    }
  }
  class FakeTerminal {
    static instances: FakeTerminal[] = [];
    options: Record<string, unknown>;
    cols = 80;
    rows = 24;
    unicode = { activeVersion: "6" };
    element: HTMLElement | null = null;
    written: Array<string | Uint8Array> = [];
    selection = "";
    pasted: string[] = [];
    keyHandler: ((event: KeyboardEvent) => boolean) | null = null;
    disposed = false;
    data = new Emitter<string>();
    binary = new Emitter<string>();
    resize = new Emitter<{ cols: number; rows: number }>();
    selectionChange = new Emitter<void>();
    onData = this.data.event;
    onBinary = this.binary.event;
    onResize = this.resize.event;
    onSelectionChange = this.selectionChange.event;
    constructor(options: Record<string, unknown>) {
      this.options = { ...options };
      FakeTerminal.instances.push(this);
    }
    loadAddon(addon: { activate(terminal: unknown): void }) {
      addon.activate(this);
    }
    open(element: HTMLElement) {
      this.element = element;
    }
    write(data: string | Uint8Array, callback?: () => void) {
      this.written.push(data);
      callback?.();
    }
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      this.keyHandler = handler;
    }
    hasSelection() {
      return this.selection !== "";
    }
    getSelection() {
      return this.selection;
    }
    paste(text: string) {
      this.pasted.push(text);
    }
    focus() {}
    dispose() {
      this.disposed = true;
    }
  }
  class FakeWebgl {
    static instances: FakeWebgl[] = [];
    static failNext = false;
    disposed = false;
    contextLoss = new Emitter<void>();
    onContextLoss = this.contextLoss.event;
    constructor() {
      if (FakeWebgl.failNext) {
        FakeWebgl.failNext = false;
        throw new Error("WebGL2 not supported");
      }
      FakeWebgl.instances.push(this);
    }
    activate() {}
    dispose() {
      this.disposed = true;
    }
  }
  class FakeFit {
    static fits = 0;
    activate() {}
    dispose() {}
    fit() {
      FakeFit.fits += 1;
    }
  }
  class FakeAddon {
    activate() {}
    dispose() {}
  }
  return { FakeTerminal, FakeWebgl, FakeFit, FakeAddon };
});

vi.mock("@xterm/xterm", () => ({ Terminal: fakes.FakeTerminal }));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: fakes.FakeWebgl }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: fakes.FakeFit }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: fakes.FakeAddon }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: fakes.FakeAddon }));
vi.mock("../../ipc/api");

class FakeResizeObserver {
  observe() {}
  disconnect() {}
}

const tab: Tab = { id: "tab-1", title: "web-prod-01", kind: "ssh", hostId: "h", sessionId: "s-1", status: "connected" };
const detach = vi.fn();
const stream = {
  channel: {},
  bind: vi.fn(),
  ack: vi.fn(),
  attach: vi.fn((onData: (bytes: Uint8Array) => void) => {
    onData(new Uint8Array([104, 105]));
    return detach;
  }),
};
const clipboard = { writeText: vi.fn(async () => undefined), readText: vi.fn(async () => "pasted") };
const term = () => fakes.FakeTerminal.instances[0] as InstanceType<typeof fakes.FakeTerminal>;
const flush = () => act(async () => undefined);

beforeEach(() => {
  fakes.FakeTerminal.instances = [];
  fakes.FakeWebgl.instances = [];
  fakes.FakeWebgl.failNext = false;
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true });
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  registerStream(tab.id, stream as unknown as api.OutputStream);
});

afterEach(() => {
  dropStream(tab.id);
  vi.unstubAllGlobals();
});

describe("TerminalView", () => {
  it("creates an opaque xterm attached to the session output with flow-control acks", () => {
    const { container } = render(<TerminalView tab={tab} visible />);
    expect(container.querySelector("[data-terminal]")).not.toBeNull();
    expect(term().options).toMatchObject({
      fontSize: 14,
      fontFamily: '"JetBrains Mono", monospace',
      allowProposedApi: true,
      allowTransparency: false,
      theme: { background: "#0B0E13" },
    });
    expect(term().unicode.activeVersion).toBe("11");
    expect(term().written).toEqual([new Uint8Array([104, 105])]);
    expect(stream.ack).toHaveBeenCalledWith(2);
    expect(fakes.FakeWebgl.instances).toHaveLength(1);
  });

  it("sends input and resizes to the session", () => {
    render(<TerminalView tab={tab} visible />);
    expect(api.sessionResize).toHaveBeenCalledWith("s-1", 80, 24);
    term().data.fire("ls\r");
    expect(api.sessionWrite).toHaveBeenCalledWith("s-1", "ls\r");
    term().binary.fire("\u0080");
    expect(api.sessionWrite).toHaveBeenLastCalledWith("s-1", new Uint8Array([0x80]));
    term().resize.fire({ cols: 100, rows: 30 });
    expect(api.sessionResize).toHaveBeenCalledWith("s-1", 100, 30);
  });

  it("copies selections and pastes via Ctrl+Shift+V and middle click", async () => {
    render(<TerminalView tab={tab} visible />);
    term().selection = "uptime";
    term().selectionChange.fire();
    expect(clipboard.writeText).toHaveBeenCalledWith("uptime");
    const handler = term().keyHandler as (event: KeyboardEvent) => boolean;
    expect(handler(new KeyboardEvent("keydown", { key: "V", ctrlKey: true, shiftKey: true }))).toBe(false);
    await flush();
    expect(term().pasted).toEqual(["pasted"]);
    expect(handler(new KeyboardEvent("keydown", { key: "C", ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(clipboard.writeText).toHaveBeenCalledTimes(2);
    expect(handler(new KeyboardEvent("keydown", { key: "c", ctrlKey: true }))).toBe(true);
    fireEvent.mouseUp(term().element as HTMLElement, { button: 1 });
    await flush();
    expect(term().pasted).toEqual(["pasted", "pasted"]);
  });

  it("keeps WebGL only while visible and falls back to the DOM renderer on context loss", () => {
    const { rerender } = render(<TerminalView tab={tab} visible />);
    const first = fakes.FakeWebgl.instances[0];
    rerender(<TerminalView tab={tab} visible={false} />);
    expect(first?.disposed).toBe(true);
    rerender(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(2);
    act(() => fakes.FakeWebgl.instances[1]?.contextLoss.fire());
    expect(fakes.FakeWebgl.instances[1]?.disposed).toBe(true);
    rerender(<TerminalView tab={tab} visible={false} />);
    rerender(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(2);
  });

  it("never loads WebGL with the DOM renderer and survives WebGL failures", () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalRenderer: "dom" } });
    const { unmount } = render(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(0);
    unmount();
    useSettingsStore.setState({ settings: DEFAULT_SETTINGS });
    fakes.FakeWebgl.failNext = true;
    render(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(0);
  });

  it("applies font size and theme changes", () => {
    render(<TerminalView tab={tab} visible />);
    act(() => useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalFontSize: 16 } }));
    expect(term().options.fontSize).toBe(16);
    act(() => useUiStore.getState().setTheme("light"));
    expect(term().options.theme).toMatchObject({ background: "#FFFFFF" });
  });

  it("detaches and disposes on unmount", () => {
    const { unmount } = render(<TerminalView tab={tab} visible />);
    unmount();
    expect(detach).toHaveBeenCalled();
    expect(term().disposed).toBe(true);
    expect(fakes.FakeWebgl.instances[0]?.disposed).toBe(true);
  });
});
```

**`ui/src/screens/terminal/TerminalScreen.test.tsx`**
```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConnectStore } from "../../stores/connect";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { useUiStore } from "../../stores/ui";
import { TerminalScreen } from "./TerminalScreen";

vi.mock("../../ipc/api");
vi.mock("./TerminalView", () => ({
  TerminalView: ({ tab, visible }: { tab: Tab; visible: boolean }) => (
    <div data-testid={`term-${tab.id}`} data-visible={String(visible)} />
  ),
}));

const openLocal = vi.fn(async () => undefined);
const closeSession = vi.fn(async () => undefined);
const start = vi.fn(async () => undefined);
const web: Tab = { id: "a", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s-a", status: "connected" };
const local: Tab = { id: "b", title: "Lokales Terminal", kind: "local", sessionId: "s-b", status: "connected" };

function seed(tabs: Tab[], activeId: string | null) {
  useSessionsStore.setState({ tabs, activeId, openLocal, closeSession });
}

beforeEach(() => {
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useUiStore.setState({ ...useUiStore.getInitialState(), view: "terminal" }, true);
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
});

describe("TerminalScreen", () => {
  it("shows an empty state that opens a local terminal", async () => {
    seed([], null);
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.getByText("Keine offenen Sitzungen")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Lokales Terminal öffnen" }));
    expect(openLocal).toHaveBeenCalledOnce();
  });

  it("renders one tab and panel per session; only the active one is visible", async () => {
    seed([web, local], "a");
    render(<TerminalScreen />);
    const tablist = screen.getByRole("tablist", { name: "Sitzungen" });
    expect(within(tablist).getByRole("tab", { name: "web-prod-01" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("term-a")).toHaveAttribute("data-visible", "true");
    expect(screen.getByTestId("term-b")).toHaveAttribute("data-visible", "false");
    expect(screen.getByTestId("term-b").closest("[role=tabpanel]")).not.toBeVisible();
    await userEvent.click(within(tablist).getByRole("tab", { name: "Lokales Terminal" }));
    expect(useSessionsStore.getState().activeId).toBe("b");
    expect(screen.getByTestId("term-b")).toHaveAttribute("data-visible", "true");
  });

  it("hides every terminal while another view is shown", () => {
    seed([web], "a");
    useUiStore.setState({ view: "hosts" });
    render(<TerminalScreen />);
    expect(screen.getByTestId("term-a")).toHaveAttribute("data-visible", "false");
  });

  it("opens local terminals and closes tabs from the tab bar", async () => {
    seed([web], "a");
    render(<TerminalScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neues lokales Terminal" }));
    expect(openLocal).toHaveBeenCalledOnce();
    await userEvent.click(screen.getByRole("button", { name: "web-prod-01 schließen" }));
    expect(closeSession).toHaveBeenCalledWith("a");
  });

  it("offers close and reconnect when an SSH session ended", async () => {
    seed([{ ...web, status: "closed" }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung beendet" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(closeSession).toHaveBeenCalledWith("a");
    expect(start).toHaveBeenCalledWith("h-web", "ssh");
  });

  it("offers only closing for ended local sessions", async () => {
    seed([{ ...local, status: "error" }], "b");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung mit Fehler beendet" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Neu verbinden" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(closeSession).toHaveBeenCalledWith("b");
  });
});
```

- [ ] **Step 6: Run them to verify they fail**

Run: `bun run test TerminalView TerminalScreen`
Expected: FAIL — `./TerminalView` cannot be resolved; the stub `TerminalScreen` has no tab list.

- [ ] **Step 7: Implement TerminalView, TabBar, SessionEnded and TerminalScreen**

**`ui/src/screens/terminal/TerminalView.tsx`**
```tsx
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { t } from "../../i18n/de";
import { sessionResize, sessionWrite } from "../../ipc/api";
import { fireAndForget } from "../../stores/connectProgress";
import type { Tab } from "../../stores/sessions";
import { useSettingsStore } from "../../stores/settings";
import { getStream } from "../../stores/streams";
import { useUiStore } from "../../stores/ui";
import { copyText, readText } from "./clipboard";
import { clipboardShortcut, shouldUseWebgl } from "./terminalConfig";
import styles from "./TerminalView.module.css";
import { TERMINAL_FONT_FAMILY, terminalTheme } from "./theme";

/**
 * One xterm.js terminal bound to a session. Output comes from the tab's `OutputStream` (acked after
 * xterm consumed it), input/resizes go to the session. WebGL is attached only while `visible`.
 */
export function TerminalView({ tab, visible }: { tab: Tab; visible: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const [webglFailed, setWebglFailed] = useState(false);
  const theme = useUiStore((state) => state.theme);
  const fontSize = useSettingsStore((state) => state.settings.terminalFontSize);
  const renderer = useSettingsStore((state) => state.settings.terminalRenderer);
  const initial = useRef({ theme, fontSize });
  initial.current = { theme, fontSize };
  const sessionId = tab.sessionId;

  // Terminal lifetime = session lifetime.
  useEffect(() => {
    const container = containerRef.current;
    if (!container || !sessionId) return undefined;
    const term = new Terminal({
      allowProposedApi: true,
      allowTransparency: false,
      cursorBlink: true,
      fontFamily: TERMINAL_FONT_FAMILY,
      fontSize: initial.current.fontSize,
      scrollback: 10_000,
      theme: terminalTheme(initial.current.theme),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon());
    term.open(container);
    termRef.current = term;
    fitRef.current = fit;

    const subscriptions = [
      term.onData((data) => fireAndForget(() => sessionWrite(sessionId, data))),
      term.onBinary((data) =>
        fireAndForget(() => sessionWrite(sessionId, Uint8Array.from(data, (char) => char.charCodeAt(0) & 0xff))),
      ),
      term.onResize(({ cols, rows }) => fireAndForget(() => sessionResize(sessionId, cols, rows))),
      term.onSelectionChange(() => {
        if (term.hasSelection()) void copyText(term.getSelection());
      }),
    ];

    const paste = () => {
      void readText().then((text) => {
        if (text) term.paste(text);
      });
    };
    term.attachCustomKeyEventHandler((event) => {
      const action = clipboardShortcut(event);
      if (!action) return true;
      if (event.type === "keydown") {
        event.preventDefault();
        if (action === "copy") {
          if (term.hasSelection()) void copyText(term.getSelection());
        } else {
          paste();
        }
      }
      return false;
    });
    // Middle click pastes the clipboard; cancelling mouseup stops WebKit's own (PRIMARY) paste.
    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== 1) return;
      event.preventDefault();
      paste();
    };
    container.addEventListener("mouseup", onMouseUp, true);

    const stream = getStream(tab.id);
    const detach = stream?.attach((bytes) => term.write(bytes, () => stream.ack(bytes.length)));

    return () => {
      detach?.();
      container.removeEventListener("mouseup", onMouseUp, true);
      for (const subscription of subscriptions) subscription.dispose();
      webglRef.current?.dispose();
      webglRef.current = null;
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [tab.id, sessionId]);

  // WebGL renderer only while visible; DOM fallback after a failure or with the "dom" setting.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    if (!shouldUseWebgl(renderer, visible, webglFailed)) {
      webglRef.current?.dispose();
      webglRef.current = null;
      return;
    }
    if (webglRef.current) return;
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        addon.dispose();
        if (webglRef.current === addon) webglRef.current = null;
        setWebglFailed(true);
      });
      term.loadAddon(addon);
      webglRef.current = addon;
    } catch {
      setWebglFailed(true);
    }
  }, [renderer, visible, webglFailed, sessionId]);

  // Fit to the container while visible (hidden terminals have no size).
  useEffect(() => {
    const container = containerRef.current;
    const fit = fitRef.current;
    const term = termRef.current;
    if (!container || !fit || !term || !sessionId || !visible) return undefined;
    const refit = () => {
      try {
        fit.fit();
      } catch {
        // not measurable yet
      }
    };
    refit();
    // The PTY was opened with an estimated size; `onResize` only fires when xterm's size changes.
    fireAndForget(() => sessionResize(sessionId, term.cols, term.rows));
    term.focus();
    const observer = new ResizeObserver(refit);
    observer.observe(container);
    return () => observer.disconnect();
  }, [visible, sessionId]);

  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.theme = terminalTheme(theme);
  }, [theme, sessionId]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontSize = fontSize;
    if (visible) {
      try {
        fitRef.current?.fit();
      } catch {
        // not measurable yet
      }
    }
  }, [fontSize, sessionId, visible]);

  return (
    <div className={styles.frame} style={{ background: terminalTheme(theme).background }} data-terminal="">
      <div ref={containerRef} className={styles.host} aria-label={t.terminal.label(tab.title)} />
    </div>
  );
}
```

**`ui/src/screens/terminal/TerminalView.module.css`**
```css
.frame {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
  padding: 8px 4px 4px 10px;
}

.host {
  flex: 1;
  min-width: 0;
  min-height: 0;
}

/* xterm adds its own viewport; keep it opaque (WebKitGTK shows artifacts with transparency). */
.host :global(.xterm-viewport) {
  background-color: inherit;
}
```

**`ui/src/screens/terminal/TabBar.tsx`**
```tsx
import { Plus, X } from "lucide-react";
import { type KeyboardEvent, useRef } from "react";
import { cx, IconButton, nextIndex } from "../../components";
import { t } from "../../i18n/de";
import { useSessionsStore } from "../../stores/sessions";
import styles from "./TabBar.module.css";

export const tabButtonId = (id: string) => `session-tab-${id}`;
export const tabPanelId = (id: string) => `session-panel-${id}`;

/** Session tabs (one per connection or local terminal) plus "+" for a new local terminal. */
export function TabBar() {
  const tabs = useSessionsStore((state) => state.tabs);
  const activeId = useSessionsStore((state) => state.activeId);
  const setActive = useSessionsStore((state) => state.setActive);
  const closeSession = useSessionsStore((state) => state.closeSession);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, tabs.length, "horizontal");
    const tab = next === null ? undefined : tabs[next];
    if (next === null || !tab) return;
    event.preventDefault();
    setActive(tab.id);
    refs.current[next]?.focus();
  };

  return (
    <div className={styles.bar}>
      <div role="tablist" aria-label={t.terminal.sessions} className={styles.tabs}>
        {tabs.map((tab, index) => {
          const active = tab.id === activeId;
          const ended = tab.status === "closed" || tab.status === "error";
          return (
            <div key={tab.id} className={cx(styles.tab, active && styles.active)} data-status={tab.status} data-kind={tab.kind}>
              <button
                ref={(element) => {
                  refs.current[index] = element;
                }}
                type="button"
                role="tab"
                id={tabButtonId(tab.id)}
                aria-selected={active}
                aria-controls={tabPanelId(tab.id)}
                tabIndex={active ? 0 : -1}
                className={styles.tabButton}
                onClick={() => setActive(tab.id)}
                onKeyDown={(event) => onKeyDown(event, index)}
              >
                <span className={styles.dot} aria-hidden="true" />
                <span className={styles.title}>{tab.title}</span>
                {ended ? <span className="visually-hidden">{` – ${t.terminal.endedSuffix}`}</span> : null}
              </button>
              <IconButton
                size="sm"
                label={t.terminal.closeTab(tab.title)}
                icon={X}
                className={styles.close}
                onClick={() => void closeSession(tab.id)}
              />
            </div>
          );
        })}
      </div>
      <IconButton label={t.terminal.newLocal} title={t.terminal.newLocalTitle} icon={Plus} onClick={() => void openLocal()} />
    </div>
  );
}
```

**`ui/src/screens/terminal/TabBar.module.css`**
```css
.bar {
  display: flex;
  flex: 0 0 auto;
  align-items: stretch;
  min-height: 48px;
  padding-right: 8px;
  border-bottom: 1px solid var(--bd);
  background: var(--s1);
}

.bar > button {
  align-self: center;
}

.tabs {
  display: flex;
  flex: 1 1 auto;
  min-width: 0;
  overflow-x: auto;
}

.tab {
  --tab-tone: var(--green);
  display: flex;
  align-items: center;
  gap: 2px;
  max-width: 240px;
  padding: 0 6px 0 14px;
  border-right: 1px solid var(--bd);
  background: transparent;
  color: var(--muted);
}

.tab[data-kind="local"] {
  --tab-tone: var(--accent);
}

.tab[data-status="closed"] {
  --tab-tone: var(--dim);
}

.tab[data-status="error"] {
  --tab-tone: var(--red);
}

.active {
  background: var(--bg);
  box-shadow: inset 0 2px 0 var(--tab-tone);
  color: var(--text);
}

.tabButton {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  height: 100%;
  padding: 0 6px 0 0;
  border: 0;
  background: transparent;
  color: inherit;
  font-size: 13px;
  cursor: pointer;
}

.dot {
  flex: 0 0 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--tab-tone);
}

.title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.close {
  opacity: 0.7;
}

.tab:hover .close,
.active .close {
  opacity: 1;
}
```

**`ui/src/screens/terminal/SessionEnded.tsx`**
```tsx
import { RefreshCw } from "lucide-react";
import { useId } from "react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { useConnectStore } from "../../stores/connect";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import styles from "./SessionEnded.module.css";

/** Overlay on a tab whose session exited: close it, or reconnect SSH sessions. */
export function SessionEnded({ tab }: { tab: Tab }) {
  const closeSession = useSessionsStore((state) => state.closeSession);
  const titleId = useId();
  const hostId = tab.kind === "ssh" ? tab.hostId : undefined;

  const reconnect = async () => {
    if (!hostId) return;
    await closeSession(tab.id);
    void useConnectStore.getState().start(hostId, "ssh");
  };

  return (
    <div className={styles.overlay}>
      <section className={styles.card} aria-labelledby={titleId}>
        <h2 id={titleId} className={styles.title}>
          {tab.status === "error" ? t.terminal.endedError : t.terminal.ended}
        </h2>
        <p className={styles.text}>{t.terminal.endedText(tab.title)}</p>
        <div className={styles.actions}>
          <Button onClick={() => void closeSession(tab.id)}>{t.common.close}</Button>
          {hostId ? (
            <Button variant="primary" icon={RefreshCw} onClick={() => void reconnect()}>
              {t.terminal.reconnect}
            </Button>
          ) : null}
        </div>
      </section>
    </div>
  );
}
```

**`ui/src/screens/terminal/SessionEnded.module.css`**
```css
.overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  padding: 24px;
  background: color-mix(in srgb, var(--bg-deep) 45%, transparent);
}

.card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  max-width: 520px;
  padding: 20px 22px;
  border: 1px solid var(--bd-strong);
  border-radius: 16px;
  background: var(--dlg);
  box-shadow: 0 18px 48px rgb(0 0 0 / 0.4);
}

.title {
  font-size: 16px;
  font-weight: 600;
}

.text {
  color: var(--text2);
  font-size: 13px;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
```

**`ui/src/screens/terminal/TerminalScreen.tsx`**
```tsx
import { Plus } from "lucide-react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { useSessionsStore } from "../../stores/sessions";
import { useUiStore } from "../../stores/ui";
import { SessionEnded } from "./SessionEnded";
import { TabBar, tabButtonId, tabPanelId } from "./TabBar";
import styles from "./TerminalScreen.module.css";
import { TerminalView } from "./TerminalView";

/** Terminal view: tab bar + one panel per session. Stays mounted while hidden (AppShell). */
export function TerminalScreen() {
  const tabs = useSessionsStore((state) => state.tabs);
  const activeId = useSessionsStore((state) => state.activeId);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const shown = useUiStore((state) => state.view === "terminal");

  return (
    <div className={styles.screen}>
      <h1 className="visually-hidden">{t.terminal.title}</h1>
      {tabs.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t.terminal.emptyTitle}</p>
          <p>{t.terminal.emptyText}</p>
          <Button variant="primary" icon={Plus} onClick={() => void openLocal()}>
            {t.terminal.openLocal}
          </Button>
        </div>
      ) : (
        <>
          <TabBar />
          <div className={styles.panels}>
            {tabs.map((tab) => (
              <div
                key={tab.id}
                role="tabpanel"
                id={tabPanelId(tab.id)}
                aria-labelledby={tabButtonId(tab.id)}
                hidden={tab.id !== activeId}
                className={styles.panel}
              >
                {tab.sessionId ? <TerminalView tab={tab} visible={shown && tab.id === activeId} /> : null}
                {tab.status === "closed" || tab.status === "error" ? <SessionEnded tab={tab} /> : null}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
```

**`ui/src/screens/terminal/TerminalScreen.module.css`**
```css
.screen {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  background: var(--bg);
}

.panels {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 0;
}

.panel {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.panel[hidden] {
  display: none;
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  max-width: 420px;
  margin: auto;
  padding: 40px;
  color: var(--muted);
  text-align: center;
}

.emptyTitle {
  color: var(--text);
  font-size: 18px;
  font-weight: 600;
}
```

- [ ] **Step 8: Run the tests, the type check and the build**

Run: `bun run test TerminalView TerminalScreen terminalConfig sessions.test stores.test AppShell useGlobalShortcuts`
Expected: PASS (TerminalView 7, TerminalScreen 6, terminalConfig 3, sessions 3, stores 10, AppShell 7, useGlobalShortcuts 6).

Run: `bun run typecheck && bun run build`
Expected: no type errors; build succeeds (`ui/dist/assets/index-*.js` ≈ 810 kB; Vite's "chunks larger than 500 kB" notice is expected for this single-bundle desktop app and harmless).

Run (manual, desktop session): `bun run tauri dev`, press `Ctrl+Shift+T`.
Expected: a "Lokales Terminal" tab with your `$SHELL` prompt; `ls --color` shows colors; resizing the window re-wraps output; selecting text and middle-clicking pastes it once; `exit` shows "Sitzung beendet" with "Schließen".

- [ ] **Step 9: Commit**

```bash
git add package.json bun.lock ui/src/screens/terminal ui/src/stores/sessions.ts ui/src/stores/sessions.test.ts ui/src/i18n/de.ts
git commit -m "feat(ui): add terminal screen with xterm.js tabs, WebGL/DOM rendering and local terminals

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 23: Settings, first run, export and import dialogs

**Files:**
- Create: `ui/src/screens/settings/transfer.ts`, `ui/src/screens/settings/Settings.module.css`
- Modify (full replacement of the stubs): `ui/src/screens/settings/SettingsDialog.tsx`, `FirstRunDialog.tsx`, `ExportDialog.tsx`, `ImportDialog.tsx`
- Modify: `ui/src/i18n/de.ts` (add `t.settings`)
- Test: `ui/src/screens/settings/transfer.test.ts`, `SettingsDialog.test.tsx`, `FirstRunDialog.test.tsx`, `ExportDialog.test.tsx`, `ImportDialog.test.tsx`

**Interfaces:**
- Consumes: `useSettingsStore.save/load` (optimistic), `useHostsStore.load`, `useUiStore.openModal/closeModal`, `api.exportFile/importPreview/importApply`, `save()`/`open()` from `@tauri-apps/plugin-dialog` (registered in Task 15, permission `dialog:default`), `OptionCards` (Task 20), `FONT_SIZE_MIN/MAX` (Task 22).
- Produces:
  - `screens/settings/transfer.ts`: `LINASCP_EXTENSION = "linascp"`, `LINASCP_FILTER`, `exportFileName(date?)`, `ensureExtension(path)`, `validateExport({ includeSecrets, passphrase, repeat }): { passphrase?; repeat? }` (min 8 chars, must match), `defaultResolutions(preview): Record<Id, Resolution>` (conflicts → `skip`), `reportMessage(report)`, `isWrongPassphrase(error)`.
  - Dialogs keep their titles: "Einstellungen", "Willkommen bei LinaSCP", "Exportieren", "Importieren".

- [ ] **Step 1: Write the failing tests**

**`ui/src/screens/settings/transfer.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { defaultResolutions, ensureExtension, exportFileName, isWrongPassphrase, reportMessage, validateExport } from "./transfer";

describe("export helpers", () => {
  it("names export files by date and enforces the extension", () => {
    expect(exportFileName(new Date(2026, 9, 11))).toBe("linascp-2026-10-11.linascp");
    expect(ensureExtension("/home/u/backup")).toBe("/home/u/backup.linascp");
    expect(ensureExtension("/home/u/backup.linascp")).toBe("/home/u/backup.linascp");
  });

  it("requires a repeated passphrase of at least 8 characters only with secrets", () => {
    expect(validateExport({ includeSecrets: false, passphrase: "", repeat: "" })).toEqual({});
    expect(validateExport({ includeSecrets: true, passphrase: "kurz", repeat: "kurz" })).toEqual({ passphrase: "Mindestens 8 Zeichen" });
    expect(validateExport({ includeSecrets: true, passphrase: "lang-genug", repeat: "lang-genu" })).toEqual({
      repeat: "Passphrasen stimmen nicht überein",
    });
    expect(validateExport({ includeSecrets: true, passphrase: "lang-genug", repeat: "lang-genug" })).toEqual({});
  });
});

describe("import helpers", () => {
  it("skips every conflict by default", () => {
    const preview = {
      hasSecrets: false,
      items: [
        { id: "h1", name: "web", kind: "host", conflict: "sameName" },
        { id: "h2", name: "db", kind: "host", conflict: null },
        { id: "p1", name: "deploy", kind: "profile", conflict: "sameId" },
      ],
    } as const;
    expect(defaultResolutions({ ...preview, items: [...preview.items] })).toEqual({ h1: "skip", p1: "skip" });
  });

  it("summarises the import report", () => {
    expect(reportMessage({ imported: 3, skipped: 1, secretsRestored: 0 })).toBe("Import abgeschlossen: 3 importiert, 1 übersprungen");
    expect(reportMessage({ imported: 1, skipped: 0, secretsRestored: 2 })).toBe(
      "Import abgeschlossen: 1 importiert, 0 übersprungen, 2 Geheimnisse wiederhergestellt",
    );
  });

  it("recognises a wrong passphrase", () => {
    expect(isWrongPassphrase({ type: "config", message: "wrong passphrase" })).toBe(true);
    expect(isWrongPassphrase({ type: "config", message: "unsupported format_version 2" })).toBe(false);
    expect(isWrongPassphrase(new Error("wrong passphrase"))).toBe(false);
  });
});
```

**`ui/src/screens/settings/SettingsDialog.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { SettingsDialog } from "./SettingsDialog";

vi.mock("../../ipc/api");

function renderDialog(settings: Partial<api.Settings> = {}) {
  useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, firstRunDone: true, ...settings }, loaded: true });
  useUiStore.getState().openModal({ type: "settings" });
  return render(
    <ToastProvider>
      <SettingsDialog />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("SettingsDialog", () => {
  it("switches the theme immediately", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Einstellungen" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Dunkel" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: "Hell" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" }));
  });

  it("asks before mirroring into ~/.ssh and can be cancelled", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    expect(screen.getByText(/setzt einmalig die Zeile »Include ~\/\.ssh\/config\.d\/linascp\.conf«/)).toHaveTextContent(
      "Vorher wird eine Sicherung unter ~/.ssh/config.linascp.bak angelegt.",
    );
    expect(api.settingsSet).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    await userEvent.click(screen.getByRole("button", { name: "Spiegeln aktivieren" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "mirror" }));
    expect(screen.queryByRole("button", { name: "Spiegeln aktivieren" })).not.toBeInTheDocument();
  });

  it("asks before removing the mirror", async () => {
    renderDialog({ sshMode: "mirror" });
    await userEvent.click(screen.getByRole("radio", { name: "Integrieren" }));
    expect(screen.getByText("LinaSCP entfernt ~/.ssh/config.d/linascp.conf und die Include-Zeile aus ~/.ssh/config.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Spiegeln beenden" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "integrate" }));
  });

  it("saves renderer, font size and compatibility rendering", async () => {
    renderDialog();
    await userEvent.selectOptions(screen.getByLabelText("Renderer"), "dom");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalRenderer: "dom" }));
    await userEvent.selectOptions(screen.getByLabelText("Schriftgröße"), "16");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalFontSize: 16 }));
    expect(screen.getByLabelText("Schriftgröße")).toHaveValue("16");
    await userEvent.click(screen.getByRole("checkbox", { name: "Kompatibilitäts-Rendering (wirkt nach Neustart)" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ compatRendering: true }));
  });

  it("opens export and import", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Exportieren …" }));
    expect(useUiStore.getState().modal).toEqual({ type: "export" });
    await userEvent.click(screen.getByRole("button", { name: "Importieren …" }));
    expect(useUiStore.getState().modal).toEqual({ type: "import" });
  });

  it("reports failed saves", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "io", message: "~/.ssh/config: permission denied" });
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    await userEvent.click(screen.getByRole("button", { name: "Spiegeln aktivieren" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
  });
});
```

**`ui/src/screens/settings/FirstRunDialog.test.tsx`**
```tsx
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { FirstRunDialog } from "./FirstRunDialog";

vi.mock("../../ipc/api");

function renderDialog() {
  useUiStore.getState().openModal({ type: "firstRun" });
  return render(
    <ToastProvider>
      <FirstRunDialog />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useSettingsStore.setState({ ...useSettingsStore.getInitialState(), loaded: true }, true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("FirstRunDialog", () => {
  it("preselects Integrieren and saves it as done", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Willkommen bei LinaSCP" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Schließen" })).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("button", { name: "Los geht's" }));
    expect(api.settingsSet).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, sshMode: "integrate", firstRunDone: true });
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
  });

  it("explains and saves Spiegeln", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    expect(screen.getByText(/~\/\.ssh\/config\.linascp\.bak/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Los geht's" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "mirror", firstRunDone: true }));
  });

  it("treats Escape as the default choice", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    await userEvent.keyboard("{Escape}");
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "integrate", firstRunDone: true }));
  });

  it("stays open when saving fails", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "store", message: "locked" });
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Los geht's" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    expect(useUiStore.getState().modal).toEqual({ type: "firstRun" });
  });
});
```

**`ui/src/screens/settings/ExportDialog.test.tsx`**
```tsx
import * as dialog from "@tauri-apps/plugin-dialog";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useUiStore } from "../../stores/ui";
import { ExportDialog } from "./ExportDialog";

vi.mock("../../ipc/api");
vi.mock("@tauri-apps/plugin-dialog");

function renderDialog() {
  useUiStore.getState().openModal({ type: "export" });
  return render(
    <ToastProvider>
      <ExportDialog />
    </ToastProvider>,
  );
}

const submit = () => screen.getByRole("button", { name: "Exportieren …" });

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.exportFile).mockResolvedValue(undefined);
  vi.mocked(dialog.save).mockResolvedValue("/home/u/backup");
});

describe("ExportDialog", () => {
  it("exports without secrets", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Exportieren" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Private Schlüsseldateien einschließen" })).not.toBeInTheDocument();
    await userEvent.click(submit());
    expect(dialog.save).toHaveBeenCalledWith({
      title: "Export speichern",
      defaultPath: expect.stringMatching(/^linascp-\d{4}-\d{2}-\d{2}\.linascp$/),
      filters: [{ name: "LinaSCP-Export", extensions: ["linascp"] }],
    });
    await waitFor(() => expect(api.exportFile).toHaveBeenCalledWith("/home/u/backup.linascp", false, false, null));
    expect(await screen.findByText("Export gespeichert: /home/u/backup.linascp")).toBeInTheDocument();
    expect(useUiStore.getState().modal).toBeNull();
  });

  it("requires a repeated passphrase of 8+ characters for secrets", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("checkbox", { name: "Geheimnisse einschließen" }));
    expect(submit()).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Passphrase"), "kurz");
    expect(screen.getByText("Mindestens 8 Zeichen")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Passphrase"), "-aber-jetzt-lang");
    await userEvent.type(screen.getByLabelText("Passphrase wiederholen"), "kurz-aber");
    expect(screen.getByText("Passphrasen stimmen nicht überein")).toBeInTheDocument();
    expect(submit()).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Passphrase wiederholen"), "-jetzt-lang");
    expect(submit()).toBeEnabled();
    await userEvent.click(screen.getByRole("checkbox", { name: "Private Schlüsseldateien einschließen" }));
    await userEvent.click(submit());
    await waitFor(() =>
      expect(api.exportFile).toHaveBeenCalledWith("/home/u/backup.linascp", true, true, "kurz-aber-jetzt-lang"),
    );
  });

  it("does nothing when the save dialog is cancelled", async () => {
    vi.mocked(dialog.save).mockResolvedValue(null);
    renderDialog();
    await userEvent.click(submit());
    expect(api.exportFile).not.toHaveBeenCalled();
    expect(useUiStore.getState().modal).toEqual({ type: "export" });
  });

  it("reports failures", async () => {
    vi.mocked(api.exportFile).mockRejectedValue({ type: "io", message: "read-only file system" });
    renderDialog();
    await userEvent.click(submit());
    expect(await screen.findByRole("alert")).toHaveTextContent("Export fehlgeschlagen");
    expect(useUiStore.getState().modal).toEqual({ type: "export" });
  });
});
```

**`ui/src/screens/settings/ImportDialog.test.tsx`**
```tsx
import * as dialog from "@tauri-apps/plugin-dialog";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { ImportDialog } from "./ImportDialog";

vi.mock("../../ipc/api");
vi.mock("@tauri-apps/plugin-dialog");

const preview: api.ImportPreview = {
  hasSecrets: true,
  items: [
    { id: "h1", name: "web-prod-01", kind: "host", conflict: "sameName" },
    { id: "h2", name: "db-prod-01", kind: "host", conflict: null },
    { id: "p1", name: "deploy", kind: "profile", conflict: "sameId" },
  ],
};

function renderDialog() {
  useUiStore.getState().openModal({ type: "import" });
  return render(
    <ToastProvider>
      <ImportDialog />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(dialog.open).mockResolvedValue("/home/u/backup.linascp");
  vi.mocked(api.importPreview).mockResolvedValue(preview);
  vi.mocked(api.importApply).mockResolvedValue({ imported: 2, skipped: 1, secretsRestored: 3 });
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
  vi.mocked(api.settingsGet).mockResolvedValue(DEFAULT_SETTINGS);
});

describe("ImportDialog", () => {
  it("previews conflicts, applies the chosen resolutions and reloads", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Importieren" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(dialog.open).toHaveBeenCalledWith({
      title: "Import-Datei wählen",
      multiple: false,
      directory: false,
      filters: [{ name: "LinaSCP-Export", extensions: ["linascp"] }],
    });
    expect(api.importPreview).toHaveBeenCalledWith("/home/u/backup.linascp");
    expect(await screen.findByText("3 Einträge, davon 2 mit Konflikt")).toBeInTheDocument();
    const web = screen.getByRole("radiogroup", { name: "Konflikt lösen für web-prod-01" });
    expect(within(web).getByRole("radio", { name: "Überspringen" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(within(web).getByRole("radio", { name: "Ersetzen" }));
    expect(screen.getByText("gleicher Name")).toBeInTheDocument();
    expect(screen.getByText("gleiche ID")).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Konflikt lösen für db-prod-01" })).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Passphrase"), "lange-passphrase");
    await userEvent.click(screen.getByRole("button", { name: "Importieren" }));
    expect(api.importApply).toHaveBeenCalledWith("/home/u/backup.linascp", "lange-passphrase", { h1: "replace", p1: "skip" });
    expect(await screen.findByText("Import abgeschlossen: 2 importiert, 1 übersprungen, 3 Geheimnisse wiederhergestellt")).toBeInTheDocument();
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
    expect(api.hostsList).toHaveBeenCalled();
    expect(api.settingsGet).toHaveBeenCalled();
    expect(useSettingsStore.getState().loaded).toBe(true);
  });

  it("imports without secrets when the passphrase stays empty", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    await userEvent.click(await screen.findByRole("button", { name: "Importieren" }));
    expect(api.importApply).toHaveBeenCalledWith("/home/u/backup.linascp", null, { h1: "skip", p1: "skip" });
  });

  it("shows a wrong passphrase inline and stays open", async () => {
    vi.mocked(api.importApply).mockRejectedValue({ type: "config", message: "wrong passphrase" });
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    await userEvent.type(await screen.findByLabelText("Passphrase"), "falsch-falsch");
    await userEvent.click(screen.getByRole("button", { name: "Importieren" }));
    expect(await screen.findByText("Falsche Passphrase")).toBeInTheDocument();
    expect(screen.getByLabelText("Passphrase")).toHaveAttribute("aria-invalid", "true");
    expect(useUiStore.getState().modal).toEqual({ type: "import" });
  });

  it("stays on the file step when the dialog is cancelled or the file is unreadable", async () => {
    vi.mocked(dialog.open).mockResolvedValueOnce(null);
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(api.importPreview).not.toHaveBeenCalled();
    vi.mocked(api.importPreview).mockRejectedValueOnce({ type: "config", message: "not a LinaSCP export" });
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Die Datei konnte nicht gelesen werden");
    expect(screen.getByRole("button", { name: "Datei wählen …" })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `bun run test transfer SettingsDialog FirstRunDialog ExportDialog ImportDialog`
Expected: FAIL — `./transfer` cannot be resolved; the stub dialogs only show "Folgt in einer späteren Version".

- [ ] **Step 3: Implement the helpers, strings and dialogs**

**`ui/src/screens/settings/transfer.ts`**
```ts
import { t } from "../../i18n/de";
import type { Id, ImportPreview, ImportReport, Resolution } from "../../ipc/api";
import { isCoreError } from "../../ipc/errors";

export const LINASCP_EXTENSION = "linascp";
export const LINASCP_FILTER = { name: t.settings.export.filter, extensions: [LINASCP_EXTENSION] };
export const MIN_PASSPHRASE_LENGTH = 8;

const pad = (value: number) => String(value).padStart(2, "0");

export function exportFileName(date: Date = new Date()): string {
  return `linascp-${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}.${LINASCP_EXTENSION}`;
}

/** The GTK save dialog does not append the filter's extension. */
export function ensureExtension(path: string): string {
  return path.endsWith(`.${LINASCP_EXTENSION}`) ? path : `${path}.${LINASCP_EXTENSION}`;
}

export interface ExportForm {
  includeSecrets: boolean;
  passphrase: string;
  repeat: string;
}

export function validateExport(form: ExportForm): { passphrase?: string; repeat?: string } {
  if (!form.includeSecrets) return {};
  if (form.passphrase.length < MIN_PASSPHRASE_LENGTH) return { passphrase: t.settings.export.tooShort };
  if (form.repeat !== form.passphrase) return { repeat: t.settings.export.mismatch };
  return {};
}

/** Every conflicting item starts as "Überspringen"; items without conflict are imported as-is. */
export function defaultResolutions(preview: ImportPreview): Record<Id, Resolution> {
  return Object.fromEntries(preview.items.filter((item) => item.conflict !== null).map((item) => [item.id, "skip" as const]));
}

export function reportMessage(report: ImportReport): string {
  return t.settings.import.report(report.imported, report.skipped, report.secretsRestored);
}

/** Core's import rejects a wrong passphrase with `Config { message: "wrong passphrase" }`. */
export function isWrongPassphrase(error: unknown): boolean {
  return isCoreError(error) && error.type === "config" && error.message === "wrong passphrase";
}
```

**`ui/src/i18n/de.ts`** (insert before `} as const;`)
```ts
  settings: {
    title: "Einstellungen",
    done: "Fertig",
    appearance: "Darstellung",
    theme: "Design",
    themes: { dark: "Dunkel", light: "Hell" },
    ssh: "~/.ssh-Integration",
    sshMode: "ssh-Modus",
    modes: { integrate: "Integrieren", mirror: "Spiegeln" },
    integrateHint:
      "~/.ssh/config wird gelesen; Hosts daraus erscheinen mit dem Hinweis »aus ssh-config«. Die Datei wird nie verändert.",
    mirrorHint: "Zusätzlich schreibt LinaSCP seine Hosts nach ~/.ssh/config.d/linascp.conf, damit auch ssh sie kennt.",
    mirrorConfirm:
      "LinaSCP schreibt alle eigenen Hosts nach ~/.ssh/config.d/linascp.conf und setzt einmalig die Zeile »Include ~/.ssh/config.d/linascp.conf« an den Anfang von ~/.ssh/config. Vorher wird eine Sicherung unter ~/.ssh/config.linascp.bak angelegt.",
    unmirrorConfirm: "LinaSCP entfernt ~/.ssh/config.d/linascp.conf und die Include-Zeile aus ~/.ssh/config.",
    enableMirror: "Spiegeln aktivieren",
    disableMirror: "Spiegeln beenden",
    terminal: "Terminal",
    renderer: "Renderer",
    renderers: { auto: "Automatisch (WebGL, Fallback DOM)", webgl: "WebGL", dom: "DOM" },
    fontSize: "Schriftgröße",
    fontSizeOption: (size: number) => `${size} px`,
    compat: "Kompatibilitäts-Rendering (wirkt nach Neustart)",
    compatHint: "Setzt WEBKIT_DISABLE_DMABUF_RENDERER=1. Nur bei Darstellungsfehlern aktivieren.",
    data: "Daten",
    dataHint: "Hosts, Gruppen, Login-Profile und Einstellungen sichern oder auf ein anderes Gerät übertragen.",
    exportButton: "Exportieren …",
    importButton: "Importieren …",
    firstRun: {
      title: "Willkommen bei LinaSCP",
      subtitle: "Wie soll LinaSCP mit deiner ~/.ssh/config umgehen?",
      label: "Umgang mit ~/.ssh/config",
      integrateHint: "Empfohlen. Hosts aus ~/.ssh/config erscheinen in LinaSCP; die Datei wird nur gelesen und nie verändert.",
      mirrorHint:
        "Wie Integrieren – zusätzlich schreibt LinaSCP seine Hosts nach ~/.ssh/config.d/linascp.conf, damit auch ssh sie nutzen kann.",
      later: "Später in den Einstellungen änderbar.",
      start: "Los geht's",
    },
    export: {
      title: "Exportieren",
      intro: "Exportiert Hosts, Gruppen, Tags, Login-Profile, Port-Forwards und Einstellungen in eine .linascp-Datei.",
      includeSecrets: "Geheimnisse einschließen",
      includeSecretsHint: "Passwörter und Passphrasen werden mit einer Passphrase verschlüsselt (age).",
      passphrase: "Passphrase",
      repeat: "Passphrase wiederholen",
      includeKeys: "Private Schlüsseldateien einschließen",
      includeKeysHint: "Die Schlüsseldateien werden ebenfalls verschlüsselt eingebettet.",
      tooShort: "Mindestens 8 Zeichen",
      mismatch: "Passphrasen stimmen nicht überein",
      submit: "Exportieren …",
      saveTitle: "Export speichern",
      filter: "LinaSCP-Export",
      done: (path: string) => `Export gespeichert: ${path}`,
      failed: "Export fehlgeschlagen",
    },
    import: {
      title: "Importieren",
      intro: "Wähle eine .linascp-Datei. Vor dem Import siehst du, welche Einträge mit vorhandenen kollidieren.",
      pick: "Datei wählen …",
      pickTitle: "Import-Datei wählen",
      readFailed: "Die Datei konnte nicht gelesen werden",
      summary: (items: number, conflicts: number) =>
        `${items === 1 ? "1 Eintrag" : `${items} Einträge`}, davon ${conflicts} mit Konflikt`,
      kinds: { host: "Host", profile: "Login-Profil", group: "Gruppe" },
      conflicts: { sameId: "gleiche ID", sameName: "gleicher Name" },
      noConflict: "neu",
      resolve: (name: string) => `Konflikt lösen für ${name}`,
      resolutions: { skip: "Überspringen", replace: "Ersetzen", duplicate: "Als Duplikat" },
      passphrase: "Passphrase",
      passphraseHint: "Die Datei enthält verschlüsselte Geheimnisse. Ohne Passphrase werden nur die Einträge importiert.",
      wrongPassphrase: "Falsche Passphrase",
      submit: "Importieren",
      report: (imported: number, skipped: number, secrets: number) =>
        `Import abgeschlossen: ${imported} importiert, ${skipped} übersprungen${secrets > 0 ? `, ${secrets} Geheimnisse wiederhergestellt` : ""}`,
      failed: "Import fehlgeschlagen",
    },
  },
```

**`ui/src/screens/settings/SettingsDialog.tsx`**
```tsx
import { Download, Upload } from "lucide-react";
import { useId, useState } from "react";
import { Button, Checkbox, Dialog, Segmented, Select, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { Settings, SshMode, TerminalRenderer, Theme } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { FONT_SIZE_MAX, FONT_SIZE_MIN } from "../terminal/theme";
import styles from "./Settings.module.css";

const THEME_OPTIONS: ReadonlyArray<{ value: Theme; label: string }> = [
  { value: "dark", label: t.settings.themes.dark },
  { value: "light", label: t.settings.themes.light },
];
const MODE_OPTIONS: ReadonlyArray<{ value: SshMode; label: string }> = [
  { value: "integrate", label: t.settings.modes.integrate },
  { value: "mirror", label: t.settings.modes.mirror },
];
const RENDERER_OPTIONS: ReadonlyArray<{ value: TerminalRenderer; label: string }> = [
  { value: "auto", label: t.settings.renderers.auto },
  { value: "webgl", label: t.settings.renderers.webgl },
  { value: "dom", label: t.settings.renderers.dom },
];
const FONT_OPTIONS = Array.from({ length: FONT_SIZE_MAX - FONT_SIZE_MIN + 1 }, (_, index) => {
  const size = FONT_SIZE_MIN + index;
  return { value: String(size), label: t.settings.fontSizeOption(size) };
});

/** Minimal TP1 settings; every change is saved immediately (mirror changes after confirmation). */
export function SettingsDialog() {
  const settings = useSettingsStore((state) => state.settings);
  const save = useSettingsStore((state) => state.save);
  const closeModal = useUiStore((state) => state.closeModal);
  const openModal = useUiStore((state) => state.openModal);
  const toast = useToast();
  const [pendingMode, setPendingMode] = useState<SshMode | null>(null);
  const ids = { appearance: useId(), ssh: useId(), terminal: useId(), data: useId() };

  const persist = (patch: Partial<Settings>) => {
    save(patch).catch((error: unknown) =>
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) }),
    );
  };

  const confirmMode = () => {
    if (pendingMode) persist({ sshMode: pendingMode });
    setPendingMode(null);
  };

  return (
    <Dialog
      open
      size="md"
      title={t.settings.title}
      onClose={closeModal}
      footer={
        <Button variant="primary" onClick={closeModal}>
          {t.settings.done}
        </Button>
      }
    >
      <div className={styles.sections}>
        <section className={styles.section} aria-labelledby={ids.appearance}>
          <h3 id={ids.appearance} className={styles.heading}>
            {t.settings.appearance}
          </h3>
          <div className={styles.row}>
            <span className={styles.rowLabel}>{t.settings.theme}</span>
            <Segmented label={t.settings.theme} value={settings.theme} options={THEME_OPTIONS} onChange={(theme) => persist({ theme })} />
          </div>
        </section>
        <section className={styles.section} aria-labelledby={ids.ssh}>
          <h3 id={ids.ssh} className={styles.heading}>
            {t.settings.ssh}
          </h3>
          <div className={styles.row}>
            <span className={styles.rowLabel}>{t.settings.sshMode}</span>
            <Segmented
              label={t.settings.sshMode}
              value={pendingMode ?? settings.sshMode}
              options={MODE_OPTIONS}
              onChange={(mode) => setPendingMode(mode === settings.sshMode ? null : mode)}
            />
          </div>
          <p className={styles.hint}>{settings.sshMode === "mirror" ? t.settings.mirrorHint : t.settings.integrateHint}</p>
          {pendingMode ? (
            <div className={styles.confirm}>
              <p>{pendingMode === "mirror" ? t.settings.mirrorConfirm : t.settings.unmirrorConfirm}</p>
              <div className={styles.confirmActions}>
                <Button variant="ghost" size="sm" onClick={() => setPendingMode(null)}>
                  {t.common.cancel}
                </Button>
                <Button variant="primary" size="sm" onClick={confirmMode}>
                  {pendingMode === "mirror" ? t.settings.enableMirror : t.settings.disableMirror}
                </Button>
              </div>
            </div>
          ) : null}
        </section>
        <section className={styles.section} aria-labelledby={ids.terminal}>
          <h3 id={ids.terminal} className={styles.heading}>
            {t.settings.terminal}
          </h3>
          <div className={styles.grid}>
            <Select
              label={t.settings.renderer}
              value={settings.terminalRenderer}
              options={RENDERER_OPTIONS}
              onChange={(terminalRenderer) => persist({ terminalRenderer })}
            />
            <Select
              label={t.settings.fontSize}
              value={String(settings.terminalFontSize)}
              options={FONT_OPTIONS}
              onChange={(size) => persist({ terminalFontSize: Number(size) })}
            />
          </div>
          <Checkbox
            label={t.settings.compat}
            hint={t.settings.compatHint}
            checked={settings.compatRendering}
            onChange={(compatRendering) => persist({ compatRendering })}
          />
        </section>
        <section className={styles.section} aria-labelledby={ids.data}>
          <h3 id={ids.data} className={styles.heading}>
            {t.settings.data}
          </h3>
          <p className={styles.hint}>{t.settings.dataHint}</p>
          <div className={styles.buttons}>
            <Button icon={Upload} onClick={() => openModal({ type: "export" })}>
              {t.settings.exportButton}
            </Button>
            <Button icon={Download} onClick={() => openModal({ type: "import" })}>
              {t.settings.importButton}
            </Button>
          </div>
        </section>
      </div>
    </Dialog>
  );
}
```

**`ui/src/screens/settings/FirstRunDialog.tsx`**
```tsx
import { useState } from "react";
import { Button, Dialog, OptionCards, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { SshMode } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import styles from "./Settings.module.css";

const OPTIONS: ReadonlyArray<{ value: SshMode; label: string; hint: string }> = [
  { value: "integrate", label: t.settings.modes.integrate, hint: t.settings.firstRun.integrateHint },
  { value: "mirror", label: t.settings.modes.mirror, hint: t.settings.firstRun.mirrorHint },
];

/** First start: choose how LinaSCP treats ~/.ssh (Integrieren preselected); saves `firstRunDone`. */
export function FirstRunDialog() {
  const save = useSettingsStore((state) => state.save);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [mode, setMode] = useState<SshMode>("integrate");
  const [saving, setSaving] = useState(false);

  const finish = async (sshMode: SshMode) => {
    setSaving(true);
    try {
      await save({ sshMode, firstRunDone: true });
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      size="md"
      hideClose
      title={t.settings.firstRun.title}
      subtitle={t.settings.firstRun.subtitle}
      onClose={() => void finish("integrate")}
      footer={
        <>
          <span className={styles.footnote}>{t.settings.firstRun.later}</span>
          <span className={styles.spacer} />
          <Button variant="primary" disabled={saving} onClick={() => void finish(mode)}>
            {t.settings.firstRun.start}
          </Button>
        </>
      }
    >
      <div className={styles.sections}>
        <OptionCards label={t.settings.firstRun.label} value={mode} options={OPTIONS} onChange={setMode} />
        {mode === "mirror" ? <p className={styles.confirm}>{t.settings.mirrorConfirm}</p> : null}
      </div>
    </Dialog>
  );
}
```

**`ui/src/screens/settings/ExportDialog.tsx`**
```tsx
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { Button, Checkbox, Dialog, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import { exportFile } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useUiStore } from "../../stores/ui";
import styles from "./Settings.module.css";
import { ensureExtension, exportFileName, LINASCP_FILTER, validateExport } from "./transfer";

export function ExportDialog() {
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [includeKeys, setIncludeKeys] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [touched, setTouched] = useState({ passphrase: false, repeat: false });
  const [busy, setBusy] = useState(false);
  const errors = validateExport({ includeSecrets, passphrase, repeat });
  const invalid = Object.keys(errors).length > 0;
  const texts = t.settings.export;

  const run = async () => {
    if (invalid) return;
    const chosen = await saveDialog({ title: texts.saveTitle, defaultPath: exportFileName(), filters: [LINASCP_FILTER] });
    if (!chosen) return;
    const path = ensureExtension(chosen);
    setBusy(true);
    try {
      await exportFile(path, includeSecrets, includeSecrets && includeKeys, includeSecrets ? passphrase : null);
      toast.show({ tone: "green", message: texts.done(path) });
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: texts.failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      size="md"
      title={texts.title}
      onClose={closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" disabled={invalid || busy} onClick={() => void run()}>
            {texts.submit}
          </Button>
        </>
      }
    >
      <div className={styles.sections}>
        <p className={styles.hint}>{texts.intro}</p>
        <Checkbox label={texts.includeSecrets} hint={texts.includeSecretsHint} checked={includeSecrets} onChange={setIncludeSecrets} />
        {includeSecrets ? (
          <>
            <div className={styles.grid}>
              <TextField
                type="password"
                label={texts.passphrase}
                autoComplete="new-password"
                value={passphrase}
                error={touched.passphrase ? errors.passphrase : null}
                onChange={(value) => {
                  setPassphrase(value);
                  setTouched((current) => ({ ...current, passphrase: true }));
                }}
              />
              <TextField
                type="password"
                label={texts.repeat}
                autoComplete="new-password"
                value={repeat}
                error={touched.repeat ? errors.repeat : null}
                onChange={(value) => {
                  setRepeat(value);
                  setTouched((current) => ({ ...current, repeat: true }));
                }}
              />
            </div>
            <Checkbox label={texts.includeKeys} hint={texts.includeKeysHint} checked={includeKeys} onChange={setIncludeKeys} />
          </>
        ) : null}
      </div>
    </Dialog>
  );
}
```

**`ui/src/screens/settings/ImportDialog.tsx`**
```tsx
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { Button, Dialog, Segmented, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import { type Id, type ImportPreview, importApply, importPreview, type Resolution } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import styles from "./Settings.module.css";
import { defaultResolutions, isWrongPassphrase, LINASCP_FILTER, reportMessage } from "./transfer";

type Stage = { step: "pick" } | { step: "review"; path: string; preview: ImportPreview };

const RESOLUTION_OPTIONS: ReadonlyArray<{ value: Resolution; label: string }> = [
  { value: "skip", label: t.settings.import.resolutions.skip },
  { value: "replace", label: t.settings.import.resolutions.replace },
  { value: "duplicate", label: t.settings.import.resolutions.duplicate },
];

export function ImportDialog() {
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [stage, setStage] = useState<Stage>({ step: "pick" });
  const [resolutions, setResolutions] = useState<Record<Id, Resolution>>({});
  const [passphrase, setPassphrase] = useState("");
  const [passphraseError, setPassphraseError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const texts = t.settings.import;

  const pick = async () => {
    const path = await openDialog({ title: texts.pickTitle, multiple: false, directory: false, filters: [LINASCP_FILTER] });
    if (typeof path !== "string") return;
    setBusy(true);
    try {
      const preview = await importPreview(path);
      setResolutions(defaultResolutions(preview));
      setStage({ step: "review", path, preview });
    } catch (error) {
      toast.show({ tone: "red", message: texts.readFailed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (stage.step !== "review") return;
    setBusy(true);
    setPassphraseError(null);
    try {
      const report = await importApply(stage.path, stage.preview.hasSecrets && passphrase ? passphrase : null, resolutions);
      toast.show({ tone: "green", message: reportMessage(report) });
      try {
        await Promise.all([useHostsStore.getState().load(), useSettingsStore.getState().load()]);
      } catch (error) {
        toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) });
      }
      closeModal();
    } catch (error) {
      if (isWrongPassphrase(error)) setPassphraseError(texts.wrongPassphrase);
      else toast.show({ tone: "red", message: texts.failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  const footer =
    stage.step === "pick" ? (
      <>
        <Button variant="ghost" onClick={closeModal}>
          {t.common.cancel}
        </Button>
        <Button variant="primary" disabled={busy} onClick={() => void pick()}>
          {texts.pick}
        </Button>
      </>
    ) : (
      <>
        <Button variant="ghost" onClick={closeModal}>
          {t.common.cancel}
        </Button>
        <Button variant="primary" disabled={busy} onClick={() => void apply()}>
          {texts.submit}
        </Button>
      </>
    );

  return (
    <Dialog open size="md" title={texts.title} onClose={closeModal} footer={footer}>
      {stage.step === "pick" ? (
        <p className={styles.hint}>{texts.intro}</p>
      ) : (
        <div className={styles.sections}>
          <p className="mono">{stage.path}</p>
          <p className={styles.hint}>
            {texts.summary(stage.preview.items.length, stage.preview.items.filter((item) => item.conflict !== null).length)}
          </p>
          <ul className={styles.items}>
            {stage.preview.items.map((item) => (
              <li key={`${item.kind}-${item.id}`} className={styles.item}>
                <span className={styles.itemKind}>{texts.kinds[item.kind]}</span>
                <span className={styles.itemName}>{item.name}</span>
                {item.conflict ? (
                  <>
                    <span className={styles.conflict}>{texts.conflicts[item.conflict]}</span>
                    <Segmented
                      size="sm"
                      label={texts.resolve(item.name)}
                      value={resolutions[item.id] ?? "skip"}
                      options={RESOLUTION_OPTIONS}
                      onChange={(resolution) => setResolutions((current) => ({ ...current, [item.id]: resolution }))}
                    />
                  </>
                ) : (
                  <span className={styles.fresh}>{texts.noConflict}</span>
                )}
              </li>
            ))}
          </ul>
          {stage.preview.hasSecrets ? (
            <TextField
              type="password"
              label={texts.passphrase}
              hint={texts.passphraseHint}
              autoComplete="off"
              value={passphrase}
              error={passphraseError}
              onChange={(value) => {
                setPassphrase(value);
                setPassphraseError(null);
              }}
            />
          ) : null}
        </div>
      )}
    </Dialog>
  );
}
```

**`ui/src/screens/settings/Settings.module.css`**
```css
.sections {
  display: flex;
  flex-direction: column;
  gap: 22px;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.heading {
  color: var(--muted);
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.rowLabel {
  color: var(--text2);
}

.grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}

.hint {
  color: var(--muted);
  font-size: 13px;
}

.confirm {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  border: 1px solid var(--amber-bd);
  border-radius: 10px;
  background: var(--amber-row);
  color: var(--text2);
  font-size: 13px;
}

.confirmActions,
.buttons {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.buttons {
  justify-content: flex-start;
}

.footnote {
  align-self: center;
  color: var(--muted);
  font-size: 12px;
}

.spacer {
  flex: 1;
}

.items {
  display: flex;
  flex-direction: column;
  max-height: 280px;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  border: 1px solid var(--bd);
  border-radius: 10px;
  list-style: none;
}

.item {
  display: grid;
  grid-template-columns: 96px minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
}

.item + .item {
  border-top: 1px solid var(--bd);
}

.itemKind {
  color: var(--muted);
  font-size: 12px;
}

.itemName {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conflict {
  color: var(--amber);
  font-size: 12px;
}

.fresh {
  grid-column: span 2;
  justify-self: end;
  color: var(--green);
  font-size: 12px;
}
```

- [ ] **Step 4: Run the tests and the type check**

Run: `bun run test transfer SettingsDialog FirstRunDialog ExportDialog ImportDialog AppShell`
Expected: PASS (transfer 5, SettingsDialog 6, FirstRunDialog 4, ExportDialog 4, ImportDialog 4, AppShell 7).

Run: `bun run typecheck`
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/settings ui/src/i18n/de.ts
git commit -m "feat(ui): add settings, first-run, export and import dialogs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 24: Packaging, README, acceptance checklist and final verification

**Files:**
- Modify (full replacement): `src-tauri/tauri.conf.json` (Linux bundle section)
- Create: `src-tauri/linascp.desktop` (desktop entry template), `src-tauri/tests/bundle_config.rs`
- Create: `README.md` (German user README), `docs/acceptance-tp1.md` (manual acceptance checklist)

**Interfaces:**
- Consumes: everything from Tasks 1–23; the Tauri bundler (`bun run tauri build`).
- Produces: `target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb`, `target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage`; desktop entry with `Categories=Network;RemoteAccess;`; deb `Depends` including `libwebkit2gtk-4.1-0, libgtk-3-0, libdbus-1-3`.

- [ ] **Step 1: Write the failing packaging test**

**`src-tauri/tests/bundle_config.rs`**
```rust
//! Packaging configuration: deb + AppImage, runtime dependencies and the desktop entry.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tauri_conf() -> Value {
    let raw = std::fs::read_to_string(manifest_dir().join("tauri.conf.json")).expect("read tauri.conf.json");
    serde_json::from_str(&raw).expect("tauri.conf.json is valid JSON")
}

fn strings(value: &Value) -> Vec<&str> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string")).collect()
}

#[test]
fn bundles_deb_and_appimage() {
    let conf = tauri_conf();
    assert_eq!(strings(&conf["bundle"]["targets"]), ["deb", "appimage"]);
    assert_eq!(conf["productName"], "LinaSCP");
}

#[test]
fn deb_declares_the_runtime_libraries() {
    let conf = tauri_conf();
    let depends = strings(&conf["bundle"]["linux"]["deb"]["depends"]);
    for package in ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libdbus-1-3"] {
        assert!(depends.contains(&package), "missing deb dependency {package}");
    }
}

#[test]
fn desktop_entry_lists_network_and_remote_access() {
    let conf = tauri_conf();
    let template = conf["bundle"]["linux"]["deb"]["desktopTemplate"].as_str().expect("desktopTemplate is set");
    let content = std::fs::read_to_string(manifest_dir().join(Path::new(template))).expect("read desktop template");
    let lines: Vec<&str> = content.lines().collect();
    assert!(lines.contains(&"[Desktop Entry]"));
    assert!(lines.contains(&"Categories=Network;RemoteAccess;"));
    assert!(lines.contains(&"Exec={{exec}}"));
    assert!(lines.contains(&"Icon={{icon}}"));
    assert!(lines.contains(&"Terminal=false"));
}
```

Run: `cargo test -p linascp --test bundle_config`
Expected: FAIL — `deb_declares_the_runtime_libraries` and `desktop_entry_lists_network_and_remote_access` panic (`expected array` / `desktopTemplate is set`); `bundles_deb_and_appimage` passes already.

- [ ] **Step 2: Bundle configuration and desktop entry**

**`src-tauri/tauri.conf.json`**
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "LinaSCP",
  "version": "0.1.0",
  "identifier": "de.mc-kg.linascp",
  "build": {
    "devUrl": "http://localhost:5173",
    "frontendDist": "../ui/dist",
    "beforeDevCommand": "bun run dev",
    "beforeBuildCommand": "bun run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "LinaSCP",
        "width": 1440,
        "height": 900,
        "minWidth": 1100,
        "minHeight": 700,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self' ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:",
      "devCsp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["deb", "appimage"],
    "category": "Network",
    "shortDescription": "SSH- und SFTP-Client für Linux",
    "longDescription": "LinaSCP ist ein SSH- und SFTP-Client für Linux mit Host-Verwaltung, Login-Profilen, Host-Key-Prüfung über ~/.ssh/known_hosts, Jump-Hosts und Terminal-Tabs.",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.png"],
    "linux": {
      "deb": {
        "depends": ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libdbus-1-3"],
        "section": "net",
        "desktopTemplate": "linascp.desktop"
      }
    }
  }
}
```

**`src-tauri/linascp.desktop`**
```ini
[Desktop Entry]
Type=Application
Name={{name}}
{{#if comment}}
Comment={{comment}}
{{/if}}
Exec={{exec}}
Icon={{icon}}
Terminal=false
Categories=Network;RemoteAccess;
Keywords=SSH;SFTP;Terminal;Remote;
StartupWMClass={{exec}}
```

The deb bundler renders this Handlebars template (variables `name`, `comment`, `exec`, `icon`); the AppImage bundler builds its AppDir from the deb data directory, so it should ship the same entry (Step 3 checks both). Tauri's own `category` field stays `"Network"` (it only allows one value and is used where no template applies). If the AppImage's embedded entry lacks `RemoteAccess`, that is cosmetic (AppImages are not installed into the menu) — note it in the acceptance checklist instead of patching the bundler.

Run: `cargo test -p linascp --test bundle_config`
Expected: PASS (3 tests).

- [ ] **Step 3: Build the packages and smoke-test them (manual)**

Run: `bun run tauri build`
Expected: the UI build runs first (`beforeBuildCommand`), then the release build; the bundler prints
`target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb` and `target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage`. (The AppImage step downloads `linuxdeploy` on first use and needs the `file` package.)

Run:
```bash
dpkg-deb -f target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb Depends
dpkg-deb --fsys-tarfile target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb | tar -xO --wildcards './usr/share/applications/*.desktop'
chmod +x target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage
(cd /tmp && "$OLDPWD"/target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage --appimage-extract '*.desktop' >/dev/null && cat squashfs-root/*.desktop && rm -rf squashfs-root)
```
Expected: `Depends` lists `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libdbus-1-3`; both desktop files contain `Categories=Network;RemoteAccess;`.

Run (manual smoke test, desktop session):
```bash
DEB=target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb
PKG=$(dpkg-deb -f "$DEB" Package)          # package name chosen by the bundler
sudo apt install "./$DEB"
gtk-launch "$(basename "$(dpkg -L "$PKG" | grep '\.desktop$')" .desktop)"
target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage
```
Expected: LinaSCP appears in the GNOME app grid under "Netzwerk"; both the installed app and the AppImage open the window "LinaSCP" (first start: "Willkommen bei LinaSCP"); `Ctrl+Shift+T` opens a working local terminal; closing the window ends the process (`pgrep -fi linascp` prints nothing). Remove the package afterwards if desired: `sudo apt remove "$PKG"`.

- [ ] **Step 4: README**

**`README.md`** (the file contains code fences, so the block below is fenced with four backticks)
````markdown
# LinaSCP

SSH- und SFTP-Client für Linux (Tauri 2 + React + Rust). SSH-Funktionen orientieren sich an Termius,
SFTP-Funktionen an WinSCP. Stand: **Teilprojekt 1** – Host-Verwaltung, Login-Profile,
Verbindungsaufbau (Passwort, Schlüssel, Agent, 2FA, Jump-Host), Host-Key-Prüfung über
`~/.ssh/known_hosts`, Terminal-Tabs (SSH und lokal), `~/.ssh`-Integration, Export/Import.
Der SFTP-Dateimanager folgt in Teilprojekt 3.

## Voraussetzungen (Debian 13 „trixie“)

```bash
sudo apt install build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libdbus-1-dev unzip docker.io
```

- **atk-Pakete:** Falls `apt` bei `libwebkit2gtk-4.1-dev` wegen nicht erfüllbarer Abhängigkeiten der
  atk-Pakete (`libatk1.0-dev`, `libatk-bridge2.0-dev`) abbricht, diese aus den Backports installieren:
  `echo 'deb http://deb.debian.org/debian trixie-backports main' | sudo tee /etc/apt/sources.list.d/backports.list`,
  `sudo apt update`, `sudo apt install -t trixie-backports libatk1.0-dev libatk-bridge2.0-dev` und danach
  die obige Liste erneut ausführen.
- **Rust** (stable, Edition 2024): `curl https://sh.rustup.rs -sSf | sh`
- **bun** (Paketverwaltung und Skripte): `curl -fsSL https://bun.sh/install | bash`
- **Node.js 20** (für Vitest; Debian-Paket `nodejs` genügt)
- Für Schlüsselbund-Zugriff einen laufenden Secret Service (GNOME Keyring oder KeePassXC).

## Entwickeln

```bash
bun install            # Frontend-Abhängigkeiten
bun run tauri dev      # App mit Hot-Reload starten (Vite auf :5173)
```

Die TypeScript-Bindings (`ui/src/ipc/bindings.ts`) werden im Debug-Start und mit
`cargo test -p linascp exports_bindings` aus den Rust-Commands neu erzeugt.

## Tests

```bash
cargo test --workspace                                                     # Rust-Unit-Tests (ohne Docker)
cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1   # SSH-Integration (Docker)
cargo clippy --workspace --all-targets -- -D warnings                      # Lints
bun run typecheck                                                          # TypeScript
bun run test                                                               # Vitest (UI)
```

Die Integrationstests starten OpenSSH-Container über Docker; der aktuelle Benutzer muss Docker
verwenden dürfen (Gruppe `docker`).

## Bauen und installieren

```bash
bun run tauri build
sudo apt install ./target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb
# oder ohne Installation:
./target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage
```

## Daten und Einstellungen

| Was | Wo |
|---|---|
| Hosts, Profile, Einstellungen | `~/.local/share/linascp/linascp.db` (SQLite) |
| Passwörter, Passphrasen | Schlüsselbund des Systems (Dienst `linascp`) |
| Host-Keys | `~/.ssh/known_hosts` (gemeinsam mit `ssh`) |
| Logs | `~/.local/state/linascp/logs/` (7 Tage) |
| Gespiegelte Hosts (Modus „Spiegeln“) | `~/.ssh/config.d/linascp.conf` + `Include`-Zeile in `~/.ssh/config` (Sicherung `~/.ssh/config.linascp.bak`) |

- **Integrieren** (Standard): `~/.ssh/config` wird nur gelesen; diese Hosts tragen den Hinweis „aus ssh-config“.
- **Spiegeln:** zusätzlich werden LinaSCP-Hosts für `ssh` nutzbar gemacht.
- **Kompatibilitäts-Rendering** (Einstellungen): setzt beim nächsten Start
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` – nur bei Darstellungsfehlern einschalten.
- **Export/Import:** `.linascp`-Datei; Geheimnisse optional mit Passphrase (age) verschlüsselt.

## Tastatur

`Ctrl+K` Suche · `Ctrl+N` neuer Host · `Ctrl+Shift+T` lokales Terminal · `Ctrl+,` Einstellungen ·
im Terminal `Ctrl+Shift+C`/`Ctrl+Shift+V` kopieren/einfügen, Markieren kopiert, Mittelklick fügt ein.
````

- [ ] **Step 5: Manual acceptance checklist**

**`docs/acceptance-tp1.md`**
```markdown
# Abnahme Teilprojekt 1 – manuelle Checkliste

Grundlage: Spec `docs/superpowers/specs/2026-10-11-linascp-tp1-design.md` (Abschnitt 12) und die
TP1-Screens des Design-Pakets (`shellport-design.zip`, „Shellport“ = LinaSCP). Jeden Screen in
**Hell** und **Dunkel** prüfen (Theme-Schalter unten in der Rail). Datum, Gerät und Build notieren.

- Datum: ____________ · Gerät/GPU: ____________ · Build: `LinaSCP_0.1.0` (deb / AppImage / dev)

## 0. Vorbereitung

- [ ] Automatische Tests grün (siehe Abschnitt 4).
- [ ] Testserver bereit: ein Server mit Passwort-Login, einer mit Schlüssel (ohne und mit Passphrase),
      einer mit 2FA/Keyboard-Interactive, ein Jump-Host davor; ein laufender `ssh-agent` mit Schlüssel.
- [ ] Frisches Profil: `~/.local/share/linascp` umbenannt, `~/.ssh/config` mit mindestens einem Host-Eintrag.

## 1. Abnahmekriterien (Spec §12)

1. Verbindungen zu echten Servern
   - [ ] Passwort (Abfrage im Lade-Screen, „Passwort im Schlüsselbund speichern“, zweiter Start ohne Abfrage)
   - [ ] Schlüssel ohne Passphrase
   - [ ] Schlüssel mit Passphrase (Abfrage inline, danach aus dem Schlüsselbund)
   - [ ] SSH-Agent
   - [ ] 2FA / Keyboard-Interactive (Prompts inline beim Schritt „Anmelden“, Echo/No-Echo korrekt)
   - [ ] Über einen Jump-Host (Schritt „Jump-Host … verbinden“ mit Ziel `user@jump:port`)
2. Host-Keys konsistent mit OpenSSH
   - [ ] Von LinaSCP vertrauter Host („Vertrauen und verbinden“) → `ssh host` fragt nicht erneut.
   - [ ] Von `ssh` bekannter Host → LinaSCP verbindet ohne Dialog (auch gehashte Einträge, `[host]:port`).
   - [ ] „Nur dieses Mal“ schreibt nichts nach `~/.ssh/known_hosts`.
   - [ ] Geänderter Key (Eintrag in `known_hosts` manipulieren) → „Host-Schlüssel hat sich geändert“, Verbindung blockiert;
         „Schlüssel ersetzen und verbinden“ erst nach Häkchen aktiv, ersetzt nur den betroffenen Eintrag.
3. `~/.ssh`-Integration
   - [ ] Erststart-Dialog: „Integrieren“ vorausgewählt; Hosts aus `~/.ssh/config` erscheinen mit „aus ssh-config“.
   - [ ] „ssh-config neu einlesen“ übernimmt Änderungen an `~/.ssh/config`.
   - [ ] Bearbeiten eines ssh-config-Hosts erzeugt einen LinaSCP-Host gleichen Namens; `~/.ssh/config` bleibt unverändert.
   - [ ] Spiegeln aktivieren (Bestätigung) → `~/.ssh/config.d/linascp.conf` + `Include` am Anfang von `~/.ssh/config`,
         Backup `~/.ssh/config.linascp.bak`; `ssh <LinaSCP-Host>` funktioniert.
   - [ ] Spiegeln beenden → Datei und `Include`-Zeile entfernt.
4. Fehler-Screens
   - [ ] Zeitüberschreitung (nicht erreichbare IP), Text nennt den Timeout des Hosts.
   - [ ] Verbindung abgelehnt (Port ohne sshd), Text nennt den Port.
   - [ ] Host nicht gefunden (Tippfehler im Namen).
   - [ ] Anmeldung fehlgeschlagen (falsches Passwort): abgelehnte Methoden, leeres Passwort → „Bitte ein Passwort eingeben.“,
         neues Passwort + „im Profil speichern“ → Verbindung klappt.
   - [ ] Host-Key abgelehnt („Abbrechen“ im Dialog) → zurück zur Übersicht mit Hinweis.
   - [ ] „Versuch n von 3“ zählt weiter; Auto-Retry wartet 2 s / 4 s / 8 s und stoppt danach.
5. Export → Import auf einem zweiten Gerät
   - [ ] Export mit „Geheimnisse einschließen“ (+ Schlüsseldateien), Passphrase doppelt, mindestens 8 Zeichen; Datei hat Rechte `0600`.
   - [ ] Import auf Gerät B: Konfliktliste, Überspringen/Ersetzen/Als Duplikat wirken; falsche Passphrase → „Falsche Passphrase“.
   - [ ] Nach dem Import verbinden die Hosts ohne erneute Passworteingabe; Schlüssel liegen in `~/.ssh/linascp/` (0600).
6. Lokale Terminals
   - [ ] `Ctrl+Shift+T` und „+“ öffnen „Lokales Terminal“ mit `$SHELL` im Home-Verzeichnis, `TERM=xterm-256color`.
   - [ ] `exit` → „Sitzung beendet“ mit „Schließen“.
7. Automatische Tests
   - [ ] Alle Suiten aus Abschnitt 4 grün, inklusive Docker-Integrationstests.

## 2. Screens (jeweils Hell und Dunkel)

| # | Screen | Hell | Dunkel | Prüfpunkte |
|---|---|---|---|---|
| 1 | Server-Übersicht | [ ] | [ ] | Gruppen mit Zählern, Login-Profile mit „verwendet von“, Filter Alle/SSH/SFTP, Suche (`Ctrl+K` fokussiert), Quick Connect `user@host:port` (auch `[::1]:2222`), Karten mit SSH/SFTP-Chips, Detail-Panel (Adresse, Port, Profil, Jump-Host, Startpfad, Port-Forwards), „Dateien“ ausgegraut mit Tooltip, Löschen mit Bestätigung, leerer Zustand |
| 2 | Editor – Host | [ ] | [ ] | Reiter Allgemein/Authentifizierung/Verbindung/SFTP, Pflichtfelder, Port 1–65535, Zahlenfelder ≥ 0, Env `KEY=VALUE`, „Speichern“ gesperrt bei Fehlern, neue Gruppe, Farbwahl, Port-Forward-Regeln, „Verbindung testen“ mit Schritten und Ergebnis |
| 3 | Editor – Login-Profil | [ ] | [ ] | Sprung aus dem Auth-Reiter, „Zurück zum Host“ behält Eingaben, gespeichertes Profil ist im Host ausgewählt, Hinweis „Änderungen gelten für alle n Hosts“ |
| 4 | Laden: SSH | [ ] | [ ] | Pulsierender Avatar, Fortschrittsbalken, Schrittliste mit Meta-Texten, „▸ Details anzeigen“ zeigt `ssh -v`-Log, Abbrechen (auch `Esc`) bricht sofort ab, Rail markiert „Terminal“ |
| 5 | Unbekannter Host-Key | [ ] | [ ] | Tab „· wartet auf Bestätigung“, SHA256/MD5-Umschalter, Kopieren → „Kopiert“, Nur dieses Mal / Vertrauen und verbinden / Abbrechen |
| 6 | Host-Key geändert | [ ] | [ ] | Roter Rahmen, Vergleich Gespeichert/Aktuell, „Technische Details“ mit OpenSSH-Warnung, „Verbindung abbrechen“ fokussiert und dominant, Ersetzen erst nach Häkchen |
| 7 | Verbindungsfehler | [ ] | [ ] | Varianten Zeitüberschreitung/Abgelehnt/Nicht gefunden mit Ursachen und Originalmeldung, „Neuer Versuch …“ während des Retrys, Auto-Retry-Häkchen, Host bearbeiten, Zur Übersicht |
| 8 | Anmeldung fehlgeschlagen | [ ] | [ ] | Tab „· nicht angemeldet“, Methodenliste, Passwortvalidierung, „Login-Profil bearbeiten“ öffnet das Profil |
| 9 | Terminal | [ ] | [ ] | Tab-Leiste (aktiver Tab mit Akzentlinie oben), deckender Hintergrund, JetBrains Mono, Größenänderung passt Zeilen an, Markieren kopiert, Mittelklick fügt genau einmal ein, `Ctrl+Shift+C/V`, Links klickbar, „Sitzung beendet“ mit Neu verbinden |
| 10 | Einstellungen / Erststart / Export / Import | [ ] | [ ] | Theme, ssh-Modus mit Bestätigungstext (Include-Zeile + Backup), Renderer Auto/WebGL/DOM, Schriftgröße, Kompatibilitäts-Rendering (wirkt nach Neustart), Export-/Import-Dialoge |

## 3. Querschnitt

- [ ] Tastatur: `Ctrl+K`, `Ctrl+N`, `Ctrl+Shift+T`, `Ctrl+,`; im Terminal bleiben `Ctrl+K`/`Ctrl+N` beim Terminal.
- [ ] Bedienung nur mit Tastatur möglich (Tab-Reihenfolge, Fokus sichtbar, Dialoge fangen den Fokus, `Esc` schließt).
- [ ] Screenreader (Orca): Icon-Buttons haben Namen, Reiter/Radiogruppen werden angesagt.
- [ ] Kontrast ≥ 4,5:1 in beiden Modi (Stichprobe mit einem Kontrastprüfer).
- [ ] Renderer: „WebGL“ flüssig; „DOM“ funktioniert; 20 Tabs öffnen und wechseln → kein schwarzer Terminal-Inhalt (WebGL nur am sichtbaren Tab).
- [ ] Durchsatz: `yes | head -c 500M` bzw. `cat` großer Datei – UI bleibt bedienbar, Speicher stabil (Flow-Control).
- [ ] Kompatibilitäts-Rendering an → nach Neustart `WEBKIT_DISABLE_DMABUF_RENDERER=1` in `/proc/<pid>/environ`.
- [ ] Keine Geheimnisse in Logs: `grep -ri '<testpasswort>' ~/.local/state/linascp/logs` findet nichts.
- [ ] Gesperrter Schlüsselbund: Verbindung trotzdem möglich (Abfrage), Hinweis „Speichern … derzeit nicht möglich“.

## 4. Automatische Tests (Referenz)

- [ ] `cargo test --workspace`
- [ ] `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `bun run typecheck`
- [ ] `bun run test`

## 5. Paketierung

- [ ] `.deb` installiert, App im Startmenü unter „Netzwerk“, startet; Deinstallation sauber.
- [ ] AppImage startet auf einem zweiten Gerät ohne Installation.

Ergebnis: ☐ abgenommen ☐ Nacharbeit nötig – Notizen: ______________________________________
```

- [ ] **Step 6: Final verification (all suites)**

Run each from the repository root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1
bun run typecheck
bun run test
bun run build
```

Expected:
- `cargo fmt` / `clippy`: no output besides the summary, exit code 0.
- `cargo test --workspace`: every test binary reports `0 failed` (the Docker tests are listed as `ignored`); `bundle_config` 3 passed.
- Docker integration: all scenarios of part 20 pass (`0 failed`, `0 ignored`), Docker must be running.
- `bun run typecheck`: no errors.
- `bun run test`: `Test Files  44 passed (44)`, `Tests  227 passed (227)` (81 from Tasks 16–18 + 146 from Tasks 19–23).
- `bun run build`: `ui/dist` written (chunk-size notice only).

Then go through `docs/acceptance-tp1.md` manually with the packages from Step 3.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/linascp.desktop src-tauri/tests/bundle_config.rs README.md docs/acceptance-tp1.md
git commit -m "chore(release): package deb/AppImage with desktop entry, add README and TP1 acceptance checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

