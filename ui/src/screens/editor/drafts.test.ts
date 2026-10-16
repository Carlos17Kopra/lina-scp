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
    terminalScheme: "nord",
    terminalFontSize: 16,
    sftp: { remoteStart: "/srv", localStart: null, parallel: 3, uploadPerms: "0644", bandwidthLimitKbps: 500, sftpServerCmd: null },
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
