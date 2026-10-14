import * as dialog from "@tauri-apps/plugin-dialog";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { ImportDialog } from "./ImportDialog";

vi.mock("../../ipc/api");
vi.mock("@tauri-apps/plugin-dialog");

const preview: api.ImportPreview = {
  hasSecrets: true,
  items: [
    { id: "h1", name: "web-prod-01", kind: "host", conflict: "sameName" },
    { id: "h2", name: "db-prod-01", kind: "host", conflict: null },
    { id: "p1", name: "deploy", kind: "profile", conflict: "sameId" },
  ],
};

function renderDialog() {
  useUiStore.getState().openModal({ type: "import" });
  return render(
    <ToastProvider>
      <ImportDialog />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(dialog.open).mockResolvedValue("/home/u/backup.linascp");
  vi.mocked(api.importPreview).mockResolvedValue(preview);
  vi.mocked(api.importApply).mockResolvedValue({ imported: 2, skipped: 1, secretsRestored: 3 });
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
  vi.mocked(api.settingsGet).mockResolvedValue(DEFAULT_SETTINGS);
});

describe("ImportDialog", () => {
  it("previews conflicts, applies the chosen resolutions and reloads", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Importieren" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(dialog.open).toHaveBeenCalledWith({
      title: "Import-Datei wählen",
      multiple: false,
      directory: false,
      filters: [{ name: "LinaSCP-Export", extensions: ["linascp"] }],
    });
    expect(api.importPreview).toHaveBeenCalledWith("/home/u/backup.linascp");
    expect(await screen.findByText("3 Einträge, davon 2 mit Konflikt")).toBeInTheDocument();
    const web = screen.getByRole("radiogroup", { name: "Konflikt lösen für web-prod-01" });
    expect(within(web).getByRole("radio", { name: "Überspringen" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(within(web).getByRole("radio", { name: "Ersetzen" }));
    expect(screen.getByText("gleicher Name")).toBeInTheDocument();
    expect(screen.getByText("gleiche ID")).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Konflikt lösen für db-prod-01" })).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Passphrase"), "lange-passphrase");
    await userEvent.click(screen.getByRole("button", { name: "Importieren" }));
    expect(api.importApply).toHaveBeenCalledWith("/home/u/backup.linascp", "lange-passphrase", { h1: "replace", p1: "skip" });
    expect(await screen.findByText("Import abgeschlossen: 2 importiert, 1 übersprungen, 3 Geheimnisse wiederhergestellt")).toBeInTheDocument();
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
    expect(api.hostsList).toHaveBeenCalled();
    expect(api.settingsGet).toHaveBeenCalled();
    expect(useSettingsStore.getState().loaded).toBe(true);
  });

  it("imports without secrets when the passphrase stays empty", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    await userEvent.click(await screen.findByRole("button", { name: "Importieren" }));
    expect(api.importApply).toHaveBeenCalledWith("/home/u/backup.linascp", null, { h1: "skip", p1: "skip" });
  });

  it("shows a wrong passphrase inline and stays open", async () => {
    vi.mocked(api.importApply).mockRejectedValue({ type: "config", message: "wrong passphrase" });
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    await userEvent.type(await screen.findByLabelText("Passphrase"), "falsch-falsch");
    await userEvent.click(screen.getByRole("button", { name: "Importieren" }));
    expect(await screen.findByText("Falsche Passphrase")).toBeInTheDocument();
    expect(screen.getByLabelText("Passphrase")).toHaveAttribute("aria-invalid", "true");
    expect(useUiStore.getState().modal).toEqual({ type: "import" });
  });

  it("stays on the file step when the dialog is cancelled or the file is unreadable", async () => {
    vi.mocked(dialog.open).mockResolvedValueOnce(null);
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(api.importPreview).not.toHaveBeenCalled();
    vi.mocked(api.importPreview).mockRejectedValueOnce({ type: "config", message: "not a LinaSCP export" });
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Die Datei konnte nicht gelesen werden");
    expect(screen.getByRole("button", { name: "Datei wählen …" })).toBeInTheDocument();
  });

  it("reports a failing open dialog without an unhandled rejection", async () => {
    vi.mocked(dialog.open).mockRejectedValueOnce(new Error("portal unavailable"));
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Datei wählen …" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Dateiauswahl fehlgeschlagen");
    expect(api.importPreview).not.toHaveBeenCalled();
  });
});
