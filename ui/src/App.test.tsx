import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";
import * as api from "./ipc/api";
import { DEFAULT_SETTINGS } from "./stores/settings";

vi.mock("./ipc/api");
// xterm probes canvas support at import time, which jsdom does not implement; this test never renders a terminal.
vi.mock("./screens/terminal/TerminalView", () => ({ TerminalView: () => null }));

describe("App", () => {
  it("renders the application shell", async () => {
    vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, firstRunDone: true });
    vi.mocked(api.hostsList).mockResolvedValue([]);
    vi.mocked(api.groupsList).mockResolvedValue([]);
    vi.mocked(api.profilesList).mockResolvedValue([]);
    vi.mocked(api.snippetsList).mockResolvedValue([]);
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
  });
});
