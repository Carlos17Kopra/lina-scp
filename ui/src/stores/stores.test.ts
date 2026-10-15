import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useSessionsStore } from "./sessions";
import { makeSnippet } from "../test/fixtures";
import { DEFAULT_SETTINGS, useSettingsStore } from "./settings";
import { useSnippetsStore } from "./snippets";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

const host = (id: string, name: string) => ({ id, name }) as unknown as api.Host;

function mockLists() {
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
});

describe("ui store", () => {
  it("applies the theme class to <html>", () => {
    useUiStore.getState().setTheme("light");
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(useUiStore.getState().theme).toBe("light");
    useUiStore.getState().setTheme("dark");
    expect(document.documentElement.classList.contains("light")).toBe(false);
  });

  it("opens/closes modals and bumps the search focus token", () => {
    useUiStore.getState().openModal({ type: "editor", target: { kind: "host" } });
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host" } });
    useUiStore.getState().closeModal();
    expect(useUiStore.getState().modal).toBeNull();
    useUiStore.getState().focusSearch();
    expect(useUiStore.getState().searchFocusToken).toBe(1);
  });
});

describe("settings store", () => {
  it("loads the persisted settings", async () => {
    vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, theme: "light" });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState()).toMatchObject({ loaded: true, settings: { theme: "light" } });
  });

  it("refuses to save before the settings were loaded", async () => {
    await expect(useSettingsStore.getState().save({ theme: "light" })).rejects.toMatchObject({ type: "config" });
    expect(api.settingsSet).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().settings).toEqual(DEFAULT_SETTINGS);
  });

  it("refuses to save after loading failed", async () => {
    vi.mocked(api.settingsGet).mockRejectedValue({ type: "store", message: "locked" });
    await expect(useSettingsStore.getState().load()).rejects.toBeDefined();
    await expect(useSettingsStore.getState().save({ sshMode: "integrate" })).rejects.toMatchObject({ type: "config" });
    expect(api.settingsSet).not.toHaveBeenCalled();
  });

  it("saves optimistically and keeps the persisted result", async () => {
    useSettingsStore.setState({ loaded: true });
    vi.mocked(api.settingsSet).mockImplementation(async (s) => ({ ...s, terminalFontSize: 15 }));
    const pending = useSettingsStore.getState().save({ sshMode: "mirror" });
    expect(useSettingsStore.getState().settings.sshMode).toBe("mirror");
    await pending;
    expect(api.settingsSet).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, sshMode: "mirror" });
    expect(useSettingsStore.getState().settings.terminalFontSize).toBe(15);
  });

  it("reverts and rethrows when saving fails", async () => {
    useSettingsStore.setState({ loaded: true });
    const error = { type: "io", message: "denied" };
    vi.mocked(api.settingsSet).mockRejectedValue(error);
    await expect(useSettingsStore.getState().save({ theme: "light" })).rejects.toEqual(error);
    expect(useSettingsStore.getState().settings.theme).toBe("dark");
  });
});

describe("hosts store", () => {
  it("reloads the snippets after deleting a host (cascade)", async () => {
    mockLists();
    useSnippetsStore.setState({ snippets: [makeSnippet({ hostId: "h1" })], loaded: true });
    vi.mocked(api.snippetsList).mockResolvedValue([]);
    await useHostsStore.getState().deleteHost("h1");
    expect(useSnippetsStore.getState().snippets).toEqual([]);
  });

  it("loads hosts, groups and profiles", async () => {
    mockLists();
    vi.mocked(api.hostsList).mockResolvedValue([host("h1", "web-prod-01")]);
    await useHostsStore.getState().load();
    expect(useHostsStore.getState()).toMatchObject({ loaded: true, hosts: [{ id: "h1" }], groups: [], profiles: [] });
  });

  it("reloads after mutations and clears the selection of a deleted host", async () => {
    mockLists();
    vi.mocked(api.hostSave).mockImplementation(async (h) => h);
    vi.mocked(api.hostDelete).mockResolvedValue(undefined);
    await expect(useHostsStore.getState().saveHost(host("h2", "db"))).resolves.toMatchObject({ id: "h2" });
    expect(api.hostsList).toHaveBeenCalledTimes(1);
    useHostsStore.getState().select("h2");
    await useHostsStore.getState().deleteHost("h2");
    expect(useHostsStore.getState().selectedId).toBeNull();
    expect(api.hostsList).toHaveBeenCalledTimes(2);
  });

  it("keeps filter, query and group selection", () => {
    const s = useHostsStore.getState();
    s.setFilter("sftp");
    s.setQuery("prod");
    s.setGroupId("g1");
    expect(useHostsStore.getState()).toMatchObject({ filter: "sftp", query: "prod", groupId: "g1" });
  });
});

describe("sessions store", () => {
  it("activates new tabs and a neighbour when the active tab closes", () => {
    const s = useSessionsStore.getState();
    s.addTab({ id: "a", title: "web", kind: "ssh", status: "connecting" });
    s.addTab({ id: "b", title: "Lokal", kind: "local", status: "connected" });
    s.addTab({ id: "c", title: "db", kind: "ssh", status: "connected" });
    expect(useSessionsStore.getState().activeId).toBe("c");
    useSessionsStore.getState().setActive("b");
    useSessionsStore.getState().closeTab("b");
    expect(useSessionsStore.getState().activeId).toBe("c");
    useSessionsStore.getState().updateTab("c", { status: "closed" });
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === "c")?.status).toBe("closed");
    useSessionsStore.getState().closeTab("c");
    expect(useSessionsStore.getState().activeId).toBe("a");
    useSessionsStore.getState().closeTab("a");
    expect(useSessionsStore.getState()).toMatchObject({ tabs: [], activeId: null });
  });
});

describe("connect store", () => {
  it("starts without an active flow", () => {
    expect(useConnectStore.getState().active).toBeNull();
  });
});
