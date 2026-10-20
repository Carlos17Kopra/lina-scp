import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../components";
import * as api from "../ipc/api";
import { useSettingsStore } from "../stores/settings";
import { useUiStore } from "../stores/ui";
import { Rail } from "./Rail";

vi.mock("../ipc/api");

function renderRail() {
  return render(
    <ToastProvider>
      <Rail />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSettingsStore.setState({ ...useSettingsStore.getInitialState(), loaded: true }, true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("Rail", () => {
  it("navigates between the views", async () => {
    renderRail();
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hosts" })).toHaveAttribute("aria-current", "page");
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(useUiStore.getState().view).toBe("terminal");
    expect(screen.getByRole("button", { name: "Terminal" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Hosts" })).not.toHaveAttribute("aria-current");
    await userEvent.click(screen.getByRole("button", { name: "SFTP" }));
    expect(useUiStore.getState().view).toBe("sftp");
    await userEvent.click(screen.getByRole("button", { name: "Snippets" }));
    expect(useUiStore.getState().view).toBe("snippets");
  });

  it("opens port forwarding", async () => {
    renderRail();
    const item = screen.getByRole("button", { name: "Port-Forwarding" });
    expect(item).not.toHaveAttribute("aria-disabled");
    await userEvent.click(item);
    expect(useUiStore.getState().view).toBe("portForwarding");
  });

  it("opens the keychain", async () => {
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Schlüsselbund" }));
    expect(useUiStore.getState().view).toBe("keychain");
  });

  it("opens the settings", async () => {
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Einstellungen" }));
    expect(useUiStore.getState().view).toBe("settings");
  });

  it("toggles and persists the theme", async () => {
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Helles Design" }));
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" }));
    await userEvent.click(screen.getByRole("button", { name: "Dunkles Design" }));
    expect(document.documentElement.classList.contains("light")).toBe(false);
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ theme: "dark" }));
  });

  it("reports a failed theme save", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "io", message: "read-only" });
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Helles Design" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
  });
});
