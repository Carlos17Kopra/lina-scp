import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { FirstRunDialog } from "./FirstRunDialog";

vi.mock("../../ipc/api");

function renderDialog() {
  useUiStore.getState().openModal({ type: "firstRun" });
  return render(
    <ToastProvider>
      <FirstRunDialog />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useSettingsStore.setState({ ...useSettingsStore.getInitialState(), loaded: true }, true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("FirstRunDialog", () => {
  it("preselects Integrieren and saves it as done", async () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Willkommen bei LinaSCP" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Schließen" })).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Integrieren" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("button", { name: "Los geht's" }));
    expect(api.settingsSet).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, sshMode: "integrate", firstRunDone: true });
    await waitFor(() => expect(useUiStore.getState().modal).toBeNull());
  });

  it("explains and saves Spiegeln", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    expect(screen.getByText(/~\/\.ssh\/config\.linascp\.bak/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Los geht's" }));
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "mirror", firstRunDone: true }));
  });

  it("treats Escape as the default choice", async () => {
    renderDialog();
    await userEvent.click(screen.getByRole("radio", { name: "Spiegeln" }));
    await userEvent.keyboard("{Escape}");
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ sshMode: "integrate", firstRunDone: true }));
  });

  it("stays open when saving fails", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "store", message: "locked" });
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Los geht's" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    expect(useUiStore.getState().modal).toEqual({ type: "firstRun" });
  });
});
