import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { SettingsDialog } from "./SettingsDialog";

vi.mock("../../ipc/api");

function renderDialog(settings: Partial<api.Settings> = {}) {
  useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, firstRunDone: true, ...settings }, loaded: true });
  useUiStore.getState().openModal({ type: "settings" });
  return render(
    <ToastProvider>
      <SettingsDialog />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("SettingsDialog", () => {
  it("switches the theme immediately", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Einstellungen" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Dunkel" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: "Hell" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" }));
  });

  it("asks before mirroring into ~/.ssh and can be cancelled", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    expect(screen.getByText(/setzt einmalig die Zeile »Include ~\/\.ssh\/config\.d\/linascp\.conf«/)).toHaveTextContent(
      "Vorher wird eine Sicherung unter ~/.ssh/config.linascp.bak angelegt.",
    );
    expect(api.settingsSet).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    await userEvent.click(screen.getByRole("button", { name: "Spiegeln aktivieren" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "mirror" }));
    expect(screen.queryByRole("button", { name: "Spiegeln aktivieren" })).not.toBeInTheDocument();
  });

  it("asks before removing the mirror", async () => {
    renderDialog({ sshMode: "mirror" });
    await userEvent.click(screen.getByRole("radio", { name: "Integrieren" }));
    expect(screen.getByText("LinaSCP entfernt ~/.ssh/config.d/linascp.conf und die Include-Zeile aus ~/.ssh/config.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Spiegeln beenden" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "integrate" }));
  });

  it("saves renderer, font size and compatibility rendering", async () => {
    renderDialog();
    await userEvent.selectOptions(screen.getByLabelText("Renderer"), "dom");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalRenderer: "dom" }));
    await userEvent.selectOptions(screen.getByLabelText("Schriftgröße"), "16");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalFontSize: 16 }));
    expect(screen.getByLabelText("Schriftgröße")).toHaveValue("16");
    expect(screen.getByLabelText("Farbschema")).toHaveValue("auto");
    await userEvent.selectOptions(screen.getByLabelText("Farbschema"), "nord");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: "nord" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Kompatibilitäts-Rendering (wirkt nach Neustart)" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ compatRendering: true }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Abgebrochene SSH-Verbindungen automatisch wiederherstellen" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalAutoReconnect: false }));
  });

  it("opens export and import", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Exportieren …" }));
    expect(useUiStore.getState().modal).toEqual({ type: "export" });
    await userEvent.click(screen.getByRole("button", { name: "Importieren …" }));
    expect(useUiStore.getState().modal).toEqual({ type: "import" });
  });

  it("reports failed saves", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "io", message: "~/.ssh/config: permission denied" });
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    await userEvent.click(screen.getByRole("button", { name: "Spiegeln aktivieren" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
  });
});
