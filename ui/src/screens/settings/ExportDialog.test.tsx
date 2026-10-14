import * as dialog from "@tauri-apps/plugin-dialog";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useUiStore } from "../../stores/ui";
import { ExportDialog } from "./ExportDialog";

vi.mock("../../ipc/api");
vi.mock("@tauri-apps/plugin-dialog");

function renderDialog() {
  useUiStore.getState().openModal({ type: "export" });
  return render(
    <ToastProvider>
      <ExportDialog />
    </ToastProvider>,
  );
}

const submit = () => screen.getByRole("button", { name: "Exportieren …" });

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.exportFile).mockResolvedValue(undefined);
  vi.mocked(dialog.save).mockResolvedValue("/home/u/backup");
});

describe("ExportDialog", () => {
  it("exports without secrets", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Exportieren" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Private Schlüsseldateien einschließen" })).not.toBeInTheDocument();
    await userEvent.click(submit());
    expect(dialog.save).toHaveBeenCalledWith({
      title: "Export speichern",
      defaultPath: expect.stringMatching(/^linascp-\d{4}-\d{2}-\d{2}\.linascp$/),
      filters: [{ name: "LinaSCP-Export", extensions: ["linascp"] }],
    });
    await waitFor(() => expect(api.exportFile).toHaveBeenCalledWith("/home/u/backup.linascp", false, false, null));
    expect(await screen.findByText("Export gespeichert: /home/u/backup.linascp")).toBeInTheDocument();
    expect(useUiStore.getState().modal).toBeNull();
  });

  it("requires a repeated passphrase of 8+ characters for secrets", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("checkbox", { name: "Geheimnisse einschließen" }));
    expect(submit()).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Passphrase"), "kurz");
    expect(screen.getByText("Mindestens 8 Zeichen")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Passphrase"), "-aber-jetzt-lang");
    await userEvent.type(screen.getByLabelText("Passphrase wiederholen"), "kurz-aber");
    expect(screen.getByText("Passphrasen stimmen nicht überein")).toBeInTheDocument();
    expect(submit()).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Passphrase wiederholen"), "-jetzt-lang");
    expect(submit()).toBeEnabled();
    await userEvent.click(screen.getByRole("checkbox", { name: "Private Schlüsseldateien einschließen" }));
    await userEvent.click(submit());
    await waitFor(() =>
      expect(api.exportFile).toHaveBeenCalledWith("/home/u/backup.linascp", true, true, "kurz-aber-jetzt-lang"),
    );
  });

  it("does nothing when the save dialog is cancelled", async () => {
    vi.mocked(dialog.save).mockResolvedValue(null);
    renderDialog();
    await userEvent.click(submit());
    expect(api.exportFile).not.toHaveBeenCalled();
    expect(useUiStore.getState().modal).toEqual({ type: "export" });
  });

  it("reports failures", async () => {
    vi.mocked(api.exportFile).mockRejectedValue({ type: "io", message: "read-only file system" });
    renderDialog();
    await userEvent.click(submit());
    expect(await screen.findByRole("alert")).toHaveTextContent("Export fehlgeschlagen");
    expect(useUiStore.getState().modal).toEqual({ type: "export" });
  });

  it("reports a failing save dialog without an unhandled rejection", async () => {
    vi.mocked(dialog.save).mockRejectedValue(new Error("portal unavailable"));
    renderDialog();
    await userEvent.click(submit());
    expect(await screen.findByRole("alert")).toHaveTextContent("Dateiauswahl fehlgeschlagen");
    expect(api.exportFile).not.toHaveBeenCalled();
  });
});
