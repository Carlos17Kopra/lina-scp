import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../components";
import * as api from "../ipc/api";
import { useHostsStore } from "../stores/hosts";
import { pushNotice, useNoticeStore } from "../stores/notices";
import { DEFAULT_SETTINGS, useSettingsStore } from "../stores/settings";
import { type ModalState, useUiStore } from "../stores/ui";
import { AppShell } from "./AppShell";

vi.mock("../ipc/api");
// xterm probes canvas support at import time, which jsdom does not implement; the shell tests never render a terminal.
vi.mock("../screens/terminal/TerminalView", () => ({ TerminalView: () => null }));

function mockBackend(settings: Partial<api.Settings> = {}) {
  vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, firstRunDone: true, ...settings });
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
  vi.mocked(api.snippetsList).mockResolvedValue([]);
}

function renderShell() {
  return render(
    <ToastProvider>
      <AppShell />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
});

describe("AppShell", () => {
  it("renders the rail and the host overview and loads data", async () => {
    mockBackend();
    renderShell();
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
    await waitFor(() => expect(useSettingsStore.getState().loaded).toBe(true));
    await waitFor(() => expect(useHostsStore.getState().loaded).toBe(true));
  });

  it("switches views via the rail and keeps inactive views mounted", async () => {
    mockBackend();
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Server-Übersicht" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht", hidden: true })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "SFTP" }));
    expect(screen.getByRole("heading", { name: "SFTP" })).toBeInTheDocument();
    expect(screen.getByText("Der SFTP-Dateimanager folgt in einer späteren Version.")).toBeVisible();
  });

  it("applies the persisted theme", async () => {
    mockBackend({ theme: "light" });
    renderShell();
    await waitFor(() => expect(document.documentElement.classList.contains("light")).toBe(true));
  });

  it("opens the first-run dialog while the first run is not done", async () => {
    mockBackend({ firstRunDone: false });
    renderShell();
    expect(await screen.findByRole("dialog", { name: "Willkommen bei LinaSCP" })).toBeInTheDocument();
  });

  it("renders every modal type", async () => {
    mockBackend();
    renderShell();
    const cases: Array<[ModalState, string]> = [
      [{ type: "editor", target: { kind: "host" } }, "Neuer Host"],
      [{ type: "editor", target: { kind: "profile", id: "p1" } }, "Neues Login-Profil"],
      [{ type: "settings" }, "Einstellungen"],
      [{ type: "firstRun" }, "Willkommen bei LinaSCP"],
      [{ type: "export" }, "Exportieren"],
      [{ type: "import" }, "Importieren"],
    ];
    for (const [modal, title] of cases) {
      act(() => useUiStore.getState().openModal(modal));
      expect(screen.getByRole("dialog", { name: title })).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // Without an active connect flow the connect overlay renders nothing.
    act(() => useUiStore.getState().openModal({ type: "connect" }));
    expect(screen.queryByRole("region", { name: /Verbindung zu/ })).not.toBeInTheDocument();
  });

  it("loads the snippets on start", async () => {
    mockBackend();
    renderShell();
    await waitFor(() => expect(api.snippetsList).toHaveBeenCalled());
  });

  it("reports load failures as an error toast", async () => {
    mockBackend();
    vi.mocked(api.settingsGet).mockRejectedValue({ type: "store", message: "database is locked" });
    renderShell();
    expect(await screen.findByRole("alert")).toHaveTextContent("Daten konnten nicht geladen werden");
  });

  it("shows store notices as toasts", async () => {
    mockBackend();
    renderShell();
    act(() => pushNotice({ tone: "amber", message: "Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert." }));
    expect(await screen.findByText("Verbindung abgebrochen – der Host-Key wurde nicht akzeptiert.")).toBeInTheDocument();
    expect(useNoticeStore.getState().notices).toEqual([]);
  });
});
