import { act, render, screen, within } from "@testing-library/react";
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

describe("ConnectScreen — authentication errors at the jump host", () => {
  const jumpFailed = (hostId: string) =>
    flowFor(hostId, {
      steps: ["jumpHost", "tcp", "hostKey", "auth", "channel", "shell"],
      failed: { step: "jumpHost", error: { type: "auth", rejectedMethods: ["password"], message: "denied" } },
    });

  it("names the jump host and targets it with the password form", async () => {
    renderWith(jumpFailed("h-web"));
    expect(screen.getByText(/hat die Zugangsdaten des Jump-Hosts abgelehnt/)).toHaveTextContent(
      "Der Jump-Host »bastion« bastion.example.net hat die Zugangsdaten des Jump-Hosts abgelehnt.",
    );
    expect(screen.queryByText(/10\.0\.1\.12/)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Benutzer")).toHaveValue("admin");
    expect(screen.getByRole("checkbox", { name: "Neues Passwort für den Jump-Host »bastion« speichern" })).toBeChecked();
    await userEvent.type(screen.getByLabelText("Passwort"), "neu");
    await userEvent.click(screen.getByRole("button", { name: "Erneut anmelden" }));
    expect(actions.retryWithPassword).toHaveBeenCalledWith("neu", true);
  });

  it("edits the jump host", async () => {
    renderWith(jumpFailed("h-web"));
    await userEvent.click(screen.getByRole("button", { name: "Host bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host", id: "h-bastion" } });
  });

  it("offers only going back when the jump host is gone", async () => {
    const orphan = makeHost({ id: "h-orphan", name: "orphan", jumpHostId: "h-gone" });
    useHostsStore.setState({ hosts: [bastion, web, shop, keyHost, orphan] });
    renderWith(jumpFailed("h-orphan"));
    expect(screen.getByText("Der Jump-Host dieses Hosts ist nicht mehr vorhanden.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Passwort")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Host bearbeiten" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Erneut anmelden" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Zurück" }));
    expect(actions.cancel).toHaveBeenCalledOnce();
  });
});

describe("ConnectScreen — lifecycle", () => {
  it("moves focus into the overlay so Escape cancels", async () => {
    renderWith(flowFor("h-web", { steps: ["tcp"], current: "tcp" }));
    expect(screen.getByRole("region", { name: "Verbindung zu web-prod-01" })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(actions.cancel).toHaveBeenCalledOnce();
  });

  it("cancels the flow when its host disappears", () => {
    renderWith(flowFor("h-web", { steps: ["tcp"], current: "tcp" }));
    act(() => useHostsStore.setState({ hosts: [bastion, shop, keyHost] }));
    expect(actions.cancel).toHaveBeenCalledOnce();
    expect(screen.queryByRole("region", { name: /Verbindung zu/ })).not.toBeInTheDocument();
  });

  it("disables the retry button while a password retry is submitting", () => {
    renderWith(
      flowFor("h-shop", { submitting: true, failed: { step: "auth", error: { type: "auth", rejectedMethods: ["password"], message: "d" } } }),
    );
    expect(screen.getByRole("button", { name: "Verbinde …" })).toBeDisabled();
  });
});
