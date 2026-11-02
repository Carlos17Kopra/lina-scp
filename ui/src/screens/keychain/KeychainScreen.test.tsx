import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { KeyInfo } from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useNoticeStore } from "../../stores/notices";
import { makeHost, makeProfile, withUsage } from "../../test/fixtures";
import { KeychainScreen, pointsTo } from "./KeychainScreen";

vi.mock("../../ipc/api");
const clipboard = vi.hoisted(() => ({ writeText: vi.fn(async (_text: string) => undefined), readText: vi.fn(async () => "") }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => clipboard);

const key = (name: string, overrides: Partial<KeyInfo> = {}): KeyInfo => ({
  path: `/home/me/.ssh/${name}`,
  name,
  algorithm: "ssh-ed25519",
  fingerprint: `SHA256:${name}`,
  comment: "me@box",
  publicKey: `ssh-ed25519 AAAA${name} me@box`,
  encrypted: true,
  ...overrides,
});

const card = (name: string) => screen.getByRole("heading", { name }).closest("li") as HTMLElement;

beforeEach(() => {
  clipboard.writeText.mockClear();
  vi.mocked(api.keysList).mockResolvedValue([key("id_ed25519"), key("old", { encrypted: null, algorithm: "ssh-rsa" })]);
  vi.mocked(api.keyDefaults).mockResolvedValue({ name: "id_ed25519_linascp", comment: "me@box" });
  vi.mocked(api.keyGenerate).mockResolvedValue(key("id_ed25519_linascp"));
  vi.mocked(api.keyCopyId).mockResolvedValue(true);
  vi.mocked(api.agentIdentities).mockResolvedValue({ socket: "/run/agent.sock", identities: [] });
  vi.mocked(api.createEventChannel).mockReturnValue({ channel: 1 } as never);
  vi.mocked(api.newId).mockReturnValue("cid");
  useHostsStore.setState({
    hosts: [
      makeHost({ id: "h-web", name: "web", auth: { kind: "custom", auth: { method: "key", keyPath: "~/.ssh/id_ed25519", hasSecret: false } } }),
      makeHost({ id: "h-db", name: "db" }),
    ],
    profiles: [withUsage(makeProfile({ name: "deploy", auth: { method: "key", keyPath: "/home/me/.ssh/id_ed25519", hasSecret: false } }), 0)],
  });
});

describe("KeychainScreen", () => {
  it("lists the keys with type, fingerprint, passphrase and who uses them", async () => {
    render(<KeychainScreen />);
    const first = card(await screen.findByRole("heading", { name: "id_ed25519" }).then((h) => h.textContent ?? ""));
    expect(within(first).getByText("SHA256:id_ed25519")).toBeInTheDocument();
    expect(within(first).getByText("Mit Passphrase")).toBeInTheDocument();
    expect(within(first).getByText("Verwendet von: web, Profil deploy")).toBeInTheDocument();
    const old = card("old");
    expect(within(old).getByText("ssh-rsa")).toBeInTheDocument();
    expect(within(old).getByText("Nicht verwendet")).toBeInTheDocument();
    await userEvent.click(within(first).getByRole("button", { name: "Öffentlichen Schlüssel kopieren" }));
    expect(clipboard.writeText).toHaveBeenCalledWith("ssh-ed25519 AAAAid_ed25519 me@box");
  });

  it("matches ~ paths against the home the key lives in, not by suffix", () => {
    expect(pointsTo("~/.ssh/id_rsa", "/home/me/.ssh/id_rsa")).toBe(true);
    expect(pointsTo("~/id_rsa", "/home/me/.ssh/id_rsa")).toBe(false);
    expect(pointsTo("/home/me/.ssh/id_rsa", "/home/me/.ssh/id_rsa")).toBe(true);
  });

  it("says so when copying fails", async () => {
    clipboard.writeText.mockRejectedValueOnce(new Error("no clipboard"));
    useNoticeStore.setState(useNoticeStore.getInitialState(), true);
    render(<KeychainScreen />);
    const first = card(await screen.findByRole("heading", { name: "id_ed25519" }).then((h) => h.textContent ?? ""));
    await userEvent.click(within(first).getByRole("button", { name: "Öffentlichen Schlüssel kopieren" }));
    expect(useNoticeStore.getState().notices.map((n) => n.tone)).toEqual(["red"]);
  });

  it("shows an empty list and a failed read", async () => {
    vi.mocked(api.keysList).mockResolvedValueOnce([]);
    const { unmount } = render(<KeychainScreen />);
    expect(await screen.findByText("Keine Schlüssel in ~/.ssh gefunden.")).toBeInTheDocument();
    unmount();
    vi.mocked(api.keysList).mockRejectedValueOnce({ type: "fs", kind: "permissionDenied", path: "/home/me/.ssh", message: "denied" });
    render(<KeychainScreen />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Schlüssel konnten nicht gelesen werden: denied");
  });

  it("generates a key with a repeated passphrase and reloads the list", async () => {
    render(<KeychainScreen />);
    await screen.findByRole("heading", { name: "id_ed25519" });
    await userEvent.click(screen.getByRole("button", { name: "Neuer Schlüssel" }));
    const dialog = await screen.findByRole("dialog", { name: "Neuer Schlüssel" });
    expect(await within(dialog).findByDisplayValue("id_ed25519_linascp")).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "Kommentar" })).toHaveValue("me@box");
    await userEvent.type(within(dialog).getByLabelText("Passphrase"), "geheim");
    await userEvent.type(within(dialog).getByLabelText("Passphrase wiederholen"), "geheix");
    expect(within(dialog).getByText("Die Passphrasen stimmen nicht überein")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Erzeugen" })).toBeDisabled();
    await userEvent.type(within(dialog).getByLabelText("Passphrase wiederholen"), "{Backspace}m");
    await userEvent.click(within(dialog).getByRole("button", { name: "Erzeugen" }));
    expect(api.keyGenerate).toHaveBeenCalledWith("ed25519", "id_ed25519_linascp", "me@box", "geheim");
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(api.keysList).toHaveBeenCalledTimes(2);
  });

  it("asks for new defaults when the type changes", async () => {
    render(<KeychainScreen />);
    await screen.findByRole("heading", { name: "id_ed25519" });
    await userEvent.click(screen.getByRole("button", { name: "Neuer Schlüssel" }));
    const dialog = await screen.findByRole("dialog", { name: "Neuer Schlüssel" });
    await within(dialog).findByDisplayValue("id_ed25519_linascp");
    vi.mocked(api.keyDefaults).mockResolvedValue({ name: "id_rsa", comment: "me@box" });
    await userEvent.click(within(dialog).getByRole("radio", { name: "RSA 4096" }));
    expect(await within(dialog).findByDisplayValue("id_rsa")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Erzeugen" }));
    expect(api.keyGenerate).toHaveBeenCalledWith("rsa4096", "id_rsa", "me@box", null);
  });

  it("enters the public key on a host", async () => {
    render(<KeychainScreen />);
    const first = card(await screen.findByRole("heading", { name: "id_ed25519" }).then((h) => h.textContent ?? ""));
    await userEvent.click(within(first).getByRole("button", { name: "Auf Host eintragen …" }));
    const dialog = await screen.findByRole("dialog", { name: "id_ed25519 auf Host eintragen" });
    await userEvent.selectOptions(within(dialog).getByRole("combobox", { name: "Host" }), "h-db");
    await userEvent.click(within(dialog).getByRole("button", { name: "Eintragen" }));
    expect(api.keyCopyId).toHaveBeenCalledWith("cid", "h-db", "ssh-ed25519 AAAAid_ed25519 me@box", { channel: 1 });
    expect(await within(dialog).findByText("Schlüssel eingetragen.")).toBeInTheDocument();
  });

  it("says when the key was there already or the entry failed", async () => {
    vi.mocked(api.keyCopyId).mockResolvedValueOnce(false).mockRejectedValueOnce({ type: "auth", rejectedMethods: [], message: "nope" });
    render(<KeychainScreen />);
    const first = card(await screen.findByRole("heading", { name: "id_ed25519" }).then((h) => h.textContent ?? ""));
    await userEvent.click(within(first).getByRole("button", { name: "Auf Host eintragen …" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Eintragen" }));
    expect(await within(dialog).findByText("Der Schlüssel war schon eingetragen.")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Eintragen" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Eintragen fehlgeschlagen");
  });
});

describe("GenerateKeyDialog", () => {
  it("keeps a typed name when the type changes and cannot be closed while generating", async () => {
    let finish: (key: KeyInfo) => void = () => {};
    vi.mocked(api.keyGenerate).mockReturnValue(new Promise((resolve) => (finish = resolve)));
    render(<KeychainScreen />);
    await screen.findByRole("heading", { name: "id_ed25519" });
    await userEvent.click(screen.getByRole("button", { name: "Neuer Schlüssel" }));
    const dialog = await screen.findByRole("dialog", { name: "Neuer Schlüssel" });
    const name = await within(dialog).findByDisplayValue("id_ed25519_linascp");
    await userEvent.clear(name);
    await userEvent.type(name, "deploy");
    vi.mocked(api.keyDefaults).mockResolvedValue({ name: "id_rsa", comment: "me@box" });
    await userEvent.click(within(dialog).getByRole("radio", { name: "RSA 4096" }));
    await vi.waitFor(() => expect(api.keyDefaults).toHaveBeenLastCalledWith("rsa4096"));
    expect(within(dialog).getByRole("textbox", { name: "Dateiname" })).toHaveValue("deploy");

    await userEvent.click(within(dialog).getByRole("button", { name: "Erzeugen" }));
    expect(within(dialog).getByRole("button", { name: "Abbrechen" })).toBeDisabled();
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "Neuer Schlüssel" })).toBeInTheDocument();
    finish(key("deploy"));
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });
});

describe("KeychainScreen: SSH-Agent", () => {
  const agentSection = () => screen.getByRole("region", { name: "SSH-Agent" });

  it("lists the agent's identities, marks key files it holds and copies or enters them", async () => {
    vi.mocked(api.agentIdentities).mockResolvedValue({
      socket: "/run/agent.sock",
      identities: [
        { algorithm: "ssh-ed25519", fingerprint: "SHA256:id_ed25519", comment: "me@box", publicKey: "ssh-ed25519 AAAAagent me@box" },
        { algorithm: "ssh-rsa", fingerprint: "SHA256:card", comment: "", publicKey: "ssh-rsa AAAAcard" },
      ],
    });
    render(<KeychainScreen />);
    const section = agentSection();
    const agentCard = (await within(section).findByRole("heading", { name: "me@box" })).closest("li") as HTMLElement;
    expect(within(section).getByRole("heading", { name: "SHA256:card" })).toBeInTheDocument();
    expect(within(card("id_ed25519")).getByText("Im SSH-Agent geladen")).toBeInTheDocument();
    expect(within(card("old")).queryByText("Im SSH-Agent geladen")).not.toBeInTheDocument();

    await userEvent.click(within(agentCard).getByRole("button", { name: "Öffentlichen Schlüssel kopieren" }));
    expect(clipboard.writeText).toHaveBeenCalledWith("ssh-ed25519 AAAAagent me@box");
    await userEvent.click(within(agentCard).getByRole("button", { name: "Auf Host eintragen …" }));
    await userEvent.click(await screen.findByRole("button", { name: "Eintragen" }));
    expect(api.keyCopyId).toHaveBeenCalledWith("cid", "h-web", "ssh-ed25519 AAAAagent me@box", expect.anything());
  });

  it("says when there is no agent, when it is empty, and when it fails; reloads on request", async () => {
    vi.mocked(api.agentIdentities).mockResolvedValueOnce({ socket: null, identities: [] });
    const { unmount } = render(<KeychainScreen />);
    expect(await within(agentSection()).findByText("Kein SSH-Agent gefunden (SSH_AUTH_SOCK ist nicht gesetzt).")).toBeInTheDocument();
    unmount();

    vi.mocked(api.agentIdentities).mockRejectedValueOnce({ type: "config", message: "ssh-agent: refused" });
    render(<KeychainScreen />);
    expect(await within(agentSection()).findByRole("alert")).toHaveTextContent("ssh-agent: refused");
    await userEvent.click(within(agentSection()).getByRole("button", { name: "Neu laden" }));
    expect(await within(agentSection()).findByText("Der SSH-Agent hat keine Schlüssel geladen.")).toBeInTheDocument();
    expect(within(agentSection()).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("drops the in-agent marks when a reload fails", async () => {
    vi.mocked(api.agentIdentities).mockResolvedValueOnce({
      socket: "/run/agent.sock",
      identities: [{ algorithm: "ssh-ed25519", fingerprint: "SHA256:id_ed25519", comment: "me@box", publicKey: "ssh-ed25519 AAAAagent" }],
    });
    render(<KeychainScreen />);
    await screen.findByRole("heading", { name: "id_ed25519" });
    expect(await within(card("id_ed25519")).findByText("Im SSH-Agent geladen")).toBeInTheDocument();
    vi.mocked(api.agentIdentities).mockRejectedValueOnce({ type: "config", message: "ssh-agent: refused" });
    await userEvent.click(within(agentSection()).getByRole("button", { name: "Neu laden" }));
    expect(await within(agentSection()).findByRole("alert")).toBeInTheDocument();
    expect(within(card("id_ed25519")).queryByText("Im SSH-Agent geladen")).not.toBeInTheDocument();
  });

  it("shows a key and its certificate (same fingerprint) as two cards without key warnings", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const same = { algorithm: "ssh-ed25519", fingerprint: "SHA256:same", comment: "me@box", publicKey: "ssh-ed25519 AAAAsame" };
    vi.mocked(api.agentIdentities).mockResolvedValueOnce({ socket: "/run/agent.sock", identities: [same, same] });
    render(<KeychainScreen />);
    expect(await within(agentSection()).findAllByRole("heading", { name: "me@box" })).toHaveLength(2);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });
});
