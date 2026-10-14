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
