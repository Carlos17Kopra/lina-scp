import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
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
  it("starts a new host with the defaults from the settings", async () => {
    useSettingsStore.setState({
      settings: { ...DEFAULT_SETTINGS, newHostUser: "deploy", newHostPort: 2222, newHostKeepaliveS: 0, newHostTimeoutS: 60 },
    });
    renderEditor({ kind: "host" });
    await userEvent.type(screen.getByLabelText("Anzeigename"), "web-03");
    await userEvent.type(screen.getByLabelText("Adresse (Host oder IP)"), "10.0.0.3");
    await userEvent.click(saveButton());
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
    expect(api.hostSave).toHaveBeenCalledWith(expect.objectContaining({ user: "deploy", port: 2222, keepaliveS: 0, timeoutS: 60 }));
    useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  });

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

  it("reuses the created group when saving is retried after a failed host save", async () => {
    vi.mocked(api.hostSave).mockRejectedValueOnce({ type: "store", message: "locked" });
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.selectOptions(screen.getByLabelText("Gruppe"), "__new__");
    await userEvent.type(screen.getByLabelText("Name der neuen Gruppe"), "Kunden");
    await userEvent.click(saveButton());
    await waitFor(() => expect(api.hostSave).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    await userEvent.click(saveButton());
    await waitFor(() => expect(api.hostSave).toHaveBeenCalledTimes(2));
    expect(api.groupSave).toHaveBeenCalledTimes(1);
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ groupId: "new-id" }));
  });

  it("keeps edits made while the new group is being saved", async () => {
    let finishGroup: (group: api.Group) => void = () => undefined;
    vi.mocked(api.groupSave).mockImplementationOnce(
      () =>
        new Promise<api.Group>((resolve) => {
          finishGroup = resolve;
        }),
    );
    vi.mocked(api.hostSave).mockRejectedValueOnce({ type: "store", message: "locked" });
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.selectOptions(screen.getByLabelText("Gruppe"), "__new__");
    await userEvent.type(screen.getByLabelText("Name der neuen Gruppe"), "Kunden");
    await userEvent.click(saveButton());
    await waitFor(() => expect(api.groupSave).toHaveBeenCalledTimes(1));
    await userEvent.type(screen.getByLabelText("Anzeigename"), "-edited");
    await act(async () => finishGroup({ id: "new-id", name: "Kunden", sortOrder: 1 }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    expect(screen.getByLabelText("Anzeigename")).toHaveValue("web-prod-01-edited");
    await userEvent.click(saveButton());
    await waitFor(() => expect(api.hostSave).toHaveBeenCalledTimes(2));
    expect(api.groupSave).toHaveBeenCalledTimes(1);
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ name: "web-prod-01-edited", groupId: "new-id" }));
  });

  it("reports a failing key file dialog as a toast", async () => {
    vi.mocked(open).mockRejectedValueOnce(new Error("portal unavailable"));
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("tab", { name: "Authentifizierung" }));
    await userEvent.click(screen.getByRole("radio", { name: "Schlüssel" }));
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Dateiauswahl fehlgeschlagen");
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
        prompt: { kind: "hostKeyUnknown", host: "10.0.1.12", port: 22, info: { algorithm: "ssh-ed25519", sha256: "SHA256:abc", md5: "MD5:aa" }, otherKeyTypesKnown: [] },
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

  it("auto-answers only the first passphrase prompt of a test run", async () => {
    vi.mocked(api.connectTest).mockImplementation(() => {
      emit({ kind: "steps", steps: ["tcp", "hostKey", "auth"] });
      emit({ kind: "stepStarted", step: "auth" });
      emit({ kind: "prompt", promptId: 1, prompt: { kind: "passphrase", keyPath: "~/.ssh/id_ops" } });
      emit({ kind: "prompt", promptId: 2, prompt: { kind: "passphrase", keyPath: "~/.ssh/id_ops" } });
      return new Promise<void>(() => undefined);
    });
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("tab", { name: "Authentifizierung" }));
    await userEvent.click(screen.getByRole("radio", { name: "Schlüssel" }));
    await userEvent.type(screen.getByLabelText("Privater Schlüssel"), "~/.ssh/id_ops");
    await userEvent.type(screen.getByLabelText("Passphrase"), "wrong");
    await userEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
    expect(await screen.findByLabelText("Passphrase für ~/.ssh/id_ops")).toBeInTheDocument();
    expect(api.connectAnswer).toHaveBeenCalledTimes(1);
    expect(api.connectAnswer).toHaveBeenCalledWith(1, { kind: "secret", value: "wrong", save: false });
  });

  it("rejects a changed host key without user action", async () => {
    const info = { algorithm: "ssh-ed25519", sha256: "SHA256:abc", md5: "MD5:aa" };
    vi.mocked(api.connectTest).mockImplementation(() => {
      emit({ kind: "steps", steps: ["tcp", "hostKey", "auth"] });
      emit({ kind: "stepStarted", step: "hostKey" });
      emit({ kind: "prompt", promptId: 5, prompt: { kind: "hostKeyChanged", host: "h", port: 22, stored: info, current: info } });
      return new Promise<void>(() => undefined);
    });
    renderEditor({ kind: "host", id: "h-web" });
    await userEvent.click(screen.getByRole("button", { name: "Verbindung testen" }));
    await waitFor(() => expect(api.connectAnswer).toHaveBeenCalledWith(5, { kind: "hostKey", decision: "reject" }));
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
