import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { SettingsScreen } from "./SettingsScreen";

vi.mock("../../ipc/api");

function renderDialog(settings: Partial<api.Settings> = {}) {
  useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, firstRunDone: true, ...settings }, loaded: true });
  return render(
    <ToastProvider>
      <SettingsScreen />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("SettingsScreen", () => {
  it("switches the theme immediately", async () => {
    renderDialog();
    expect(screen.getByRole("heading", { level: 1, name: "Einstellungen" })).toBeInTheDocument();
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
    await userEvent.click(screen.getByRole("checkbox", { name: "Automatisch neu verbinden (SSH und SFTP)" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ autoReconnect: false }));
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

  it("saves the editor command on blur, empty as null", async () => {
    renderDialog();
    const field = screen.getByLabelText("Editor");
    expect(field).toHaveAttribute("placeholder", "Standard-Texteditor des Systems");
    await userEvent.type(field, " code -w ");
    await userEvent.tab();
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ editorCommand: "code -w" }));
    await userEvent.clear(field);
    await userEvent.tab();
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ editorCommand: null }));
  });

  it("saves scrollback, cursor and blinking", async () => {
    renderDialog();
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Scrollback" }), "50000");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScrollback: 50000 }));
    await userEvent.click(screen.getByRole("radio", { name: "Balken" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalCursor: "bar" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Cursor blinkt" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalCursorBlink: false }));
  });

  it("saves the local shell and the defaults for new hosts on blur", async () => {
    renderDialog();
    await userEvent.type(screen.getByRole("textbox", { name: "Shell" }), "/bin/zsh");
    await userEvent.tab();
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ localShell: "/bin/zsh" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Benutzer" }), "deploy");
    await userEvent.tab();
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ newHostUser: "deploy" }));
    const port = screen.getByRole("textbox", { name: "Port" });
    await userEvent.clear(port);
    await userEvent.type(port, "2222");
    await userEvent.tab();
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ newHostPort: 2222 }));
    const keepalive = screen.getByRole("textbox", { name: "Keepalive (s)" });
    await userEvent.clear(keepalive);
    await userEvent.type(keepalive, "0");
    await userEvent.tab();
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ newHostKeepaliveS: 0 }));
  });

  it("does not save a number that is not one and shows the backend's reason", async () => {
    vi.mocked(api.settingsSet).mockRejectedValueOnce({ type: "config", message: "Shell: /nope ist keine ausführbare Datei" });
    renderDialog();
    const timeout = screen.getByRole("textbox", { name: "Timeout (s)" });
    await userEvent.clear(timeout);
    await userEvent.type(timeout, "abc");
    await userEvent.tab();
    expect(api.settingsSet).not.toHaveBeenCalled();
    expect(timeout).toHaveValue("15");
    await userEvent.type(screen.getByRole("textbox", { name: "Shell" }), "/nope");
    await userEvent.tab();
    expect(await screen.findByText(/Speichern fehlgeschlagen/)).toBeInTheDocument();
  });
});
