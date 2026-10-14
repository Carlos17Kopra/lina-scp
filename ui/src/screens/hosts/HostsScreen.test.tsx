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

  it("ignores a second Quick Connect submit while the save is pending", async () => {
    vi.mocked(api.newId).mockReturnValue("q-1");
    let resolveSave: (host: api.Host) => void = () => undefined;
    vi.mocked(api.hostSave).mockImplementation(
      (host) =>
        new Promise<api.Host>((resolve) => {
          resolveSave = resolve;
          void host;
        }),
    );
    renderScreen();
    const quick = screen.getByRole("textbox", { name: "Quick Connect" });
    await userEvent.type(quick, "deploy@10.0.0.5{Enter}{Enter}");
    expect(api.hostSave).toHaveBeenCalledTimes(1);
    await act(async () => resolveSave(makeHost({ id: "q-1" })));
    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));
  });

  it("keeps the Quick Connect text and shows the save error when saving fails", async () => {
    vi.mocked(api.hostSave).mockRejectedValue(new Error("boom"));
    renderScreen();
    const quick = screen.getByRole("textbox", { name: "Quick Connect" });
    await userEvent.type(quick, "deploy@10.0.0.5{Enter}");
    expect(await screen.findByText("Speichern fehlgeschlagen")).toBeInTheDocument();
    expect(quick).toHaveValue("deploy@10.0.0.5");
    expect(start).not.toHaveBeenCalled();
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
