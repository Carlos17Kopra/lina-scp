import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { makeHost, makeProfile, makeSessionInfo, withUsage } from "../../../test/fixtures";
import { SessionPanel } from "./SessionPanel";

vi.mock("../../../ipc/api");
const clipboard = vi.hoisted(() => ({ writeText: vi.fn(async (_text: string) => undefined), readText: vi.fn(async () => "") }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => clipboard);

const since = new Date(2026, 9, 15, 14, 2).getTime();
const sshTab: Tab = {
  id: "t1",
  title: "web-prod-01",
  kind: "ssh",
  hostId: "h-web",
  sessionId: "s1",
  status: "connected",
  info: makeSessionInfo({ authMethods: ["key", "keyboardInteractive"] }),
  connectedAt: since,
  cols: 120,
  rows: 40,
};

function value(term: string): string {
  const row = screen.getByText(term, { selector: "dt" }).parentElement as HTMLElement;
  return within(row).getByRole("definition").textContent ?? "";
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(since + 75 * 60_000);
  const profile = makeProfile({ id: "p1", name: "deploy", user: "deploy" });
  useHostsStore.setState({
    hosts: [
      makeHost({
        id: "h-web",
        user: null,
        address: "10.0.1.12",
        port: 2222,
        jumpHostId: "h-bastion",
        keepaliveS: 30,
        auth: { kind: "profile", profileId: "p1" },
      }),
      makeHost({ id: "h-bastion", name: "bastion" }),
    ],
    profiles: [withUsage(profile, 1)],
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SessionPanel", () => {
  it("lists host, login and negotiated algorithms of an SSH session", () => {
    render(<SessionPanel tab={sshTab} />);
    expect(value("Host")).toBe("deploy@10.0.1.12:2222");
    expect(value("Jump-Host")).toBe("bastion");
    expect(value("Anmeldung")).toBe("Schlüssel + Interaktiv / 2FA · Profil deploy");
    expect(value("Host-Key")).toContain("ssh-ed25519 SHA256:abc");
    expect(value("Schlüsselaustausch")).toBe("curve25519-sha256");
    expect(value("Verschlüsselung")).toBe("chacha20-poly1305@openssh.com · none");
    expect(value("Terminal")).toBe("xterm-256color · 120×40");
    expect(value("Keepalive")).toBe("alle 30 s");
    expect(value("Verbunden seit")).toBe("14:02 (1 Std. 15 Min.)");
  });

  it("shows the user the core logs in with: host user before profile user", () => {
    useHostsStore.setState({
      hosts: [makeHost({ id: "h-web", user: "root", address: "10.0.1.12", port: 22, auth: { kind: "profile", profileId: "p1" } })],
    });
    render(<SessionPanel tab={sshTab} />);
    expect(value("Host")).toBe("root@10.0.1.12:22");
  });

  it("copies the fingerprint", async () => {
    vi.useRealTimers();
    render(<SessionPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "Fingerprint kopieren" }));
    expect(clipboard.writeText).toHaveBeenCalledWith("SHA256:abc");
  });

  it("shows dashes for missing values, a deleted host and an ended session", () => {
    const tab: Tab = { ...sshTab, hostId: "gone", status: "closed", info: makeSessionInfo({ kex: "", cipher: "", hostKeySha256: "" }) };
    render(<SessionPanel tab={tab} />);
    expect(value("Host")).toBe("—");
    expect(value("Schlüsselaustausch")).toBe("—");
    expect(value("Verschlüsselung")).toBe("—");
    expect(value("Host-Key")).toBe("—");
    expect(screen.queryByRole("button", { name: "Fingerprint kopieren" })).toBeNull();
    expect(value("Verbunden seit")).toBe("getrennt");
  });

  it("shows only shell, size and start time for local terminals", () => {
    const tab: Tab = { id: "t2", title: "Lokales Terminal", kind: "local", sessionId: "s2", status: "connected", connectedAt: since, cols: 80, rows: 24 };
    render(<SessionPanel tab={tab} />);
    expect(value("Typ")).toBe("Lokale Shell");
    expect(value("Terminal")).toBe("xterm-256color · 80×24");
    expect(value("Geöffnet seit")).toBe("14:02 (1 Std. 15 Min.)");
    expect(screen.queryByText("Host-Key")).toBeNull();
  });
});
