import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../../components";
import * as api from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../../stores/settings";
import { makeHost } from "../../../test/fixtures";
import { DesignPanel } from "./DesignPanel";

vi.mock("../../../ipc/api");

const sshTab: Tab = { id: "t1", title: "web", kind: "ssh", hostId: "h1", sessionId: "s1", status: "connected" };
const localTab: Tab = { id: "t2", title: "Lokal", kind: "local", sessionId: "s2", status: "connected" };

function renderPanel(tab: Tab) {
  return render(
    <ToastProvider>
      <DesignPanel tab={tab} />
    </ToastProvider>,
  );
}

/** Backend stand-in: `saveHost` saves, then reloads the lists. */
let stored: api.Host[] = [];

function seedHost(overrides: Partial<api.Host> = {}) {
  stored = [makeHost({ id: "h1", terminalScheme: null, terminalFontSize: null, ...overrides })];
  useHostsStore.setState({ hosts: stored });
}

beforeEach(() => {
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalFontSize: 14 }, loaded: true });
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
  vi.mocked(api.hostSave).mockImplementation(async (host) => {
    stored = stored.map((candidate) => (candidate.id === host.id ? host : candidate));
    return host;
  });
  vi.mocked(api.hostsList).mockImplementation(async () => stored);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
  seedHost();
});

describe("DesignPanel", () => {
  it("changes the global scheme and font size while 'Alle Sitzungen' is selected", async () => {
    renderPanel(sshTab);
    expect(screen.getByRole("radio", { name: "Alle Sitzungen" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: /Wie App-Design/ })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: /Nord/ }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: "nord" }));
    await userEvent.click(screen.getByRole("button", { name: "Schrift größer" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalFontSize: 15 }));
    expect(api.hostSave).not.toHaveBeenCalled();
  });

  it("copies the effective values to the host for 'Nur diesen Host' and edits the host from then on", async () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalScheme: "dracula", terminalFontSize: 13 }, loaded: true });
    renderPanel(sshTab);
    await userEvent.click(screen.getByRole("radio", { name: "Nur diesen Host" }));
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ id: "h1", terminalScheme: "dracula", terminalFontSize: 13 }));
    expect(screen.getByRole("radio", { name: "Nur diesen Host" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: /Gruvbox/ }));
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: "gruvbox", terminalFontSize: 13 }));
    expect(api.settingsSet).not.toHaveBeenCalled();
  });

  it("clears both host overrides when switching back to 'Alle Sitzungen'", async () => {
    seedHost({ terminalScheme: "nord", terminalFontSize: 18 });
    renderPanel(sshTab);
    expect(screen.getByRole("radio", { name: "Nur diesen Host" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: "Alle Sitzungen" }));
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: null, terminalFontSize: null }));
  });

  it("disables the size buttons at the limits", () => {
    seedHost({ terminalScheme: null, terminalFontSize: 20 });
    renderPanel(sshTab);
    expect(screen.getByRole("button", { name: "Schrift größer" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Schrift kleiner" })).toBeEnabled();
  });

  it("disables 'Nur diesen Host' for local tabs, ssh-config hosts and deleted hosts and says why", () => {
    const first = renderPanel(localTab);
    expect(screen.getByRole("radio", { name: "Nur diesen Host" })).toBeDisabled();
    expect(screen.getByText("Lokale Terminals verwenden immer die globalen Einstellungen.")).toBeInTheDocument();
    first.unmount();
    seedHost({ source: "sshConfig" });
    const second = renderPanel(sshTab);
    expect(screen.getByText("Hosts aus ~/.ssh/config lassen sich hier nicht einzeln anpassen.")).toBeInTheDocument();
    second.unmount();
    renderPanel({ ...sshTab, hostId: "gone" });
    expect(screen.getByText("Der Host existiert nicht mehr.")).toBeInTheDocument();
  });

  it("shows an unknown stored scheme as 'Wie App-Design'", () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalScheme: "solarizedNeon" }, loaded: true });
    renderPanel(localTab);
    expect(screen.getByRole("radio", { name: /Wie App-Design/ })).toHaveAttribute("aria-checked", "true");
  });

  it("reports a failed save and keeps the stored value", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "io", message: "disk full" });
    renderPanel(localTab);
    await userEvent.click(screen.getByRole("radio", { name: /Nord/ }));
    expect(await screen.findByText("Speichern fehlgeschlagen")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Wie App-Design/ })).toHaveAttribute("aria-checked", "true");
  });
});
