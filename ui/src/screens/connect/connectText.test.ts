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
    expect(connectView({ ...base, prompt: { promptId: 1, prompt: { kind: "hostKeyUnknown", host: "h", port: 22, info, otherKeyTypesKnown: [] } } })).toBe("hostKeyNew");
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
