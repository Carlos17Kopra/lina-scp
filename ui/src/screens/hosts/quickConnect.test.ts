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
