import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { useSessionsStore } from "../stores/sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "../stores/settings";
import { useUiStore } from "../stores/ui";
import { shortcutFor, useGlobalShortcuts } from "./useGlobalShortcuts";

vi.mock("../ipc/api");

function Harness() {
  useGlobalShortcuts();
  return (
    <div>
      <input aria-label="Suche" />
      <div data-terminal="">
        <textarea aria-label="Terminal-Eingabe" />
      </div>
    </div>
  );
}

const openLocal = vi.fn(async () => undefined);

beforeEach(() => {
  openLocal.mockClear();
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSessionsStore.setState({ ...useSessionsStore.getInitialState(), openLocal }, true);
});

describe("useGlobalShortcuts", () => {
  it("Ctrl+K shows the host overview and focuses the search", () => {
    useUiStore.getState().setView("terminal");
    render(<Harness />);
    const event = createEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    fireEvent(document.body, event);
    expect(event.defaultPrevented).toBe(true);
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", searchFocusToken: 1 });
  });

  it("Ctrl+N opens the editor for a new host", () => {
    render(<Harness />);
    fireEvent.keyDown(document.body, { key: "n", ctrlKey: true });
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host" } });
  });

  it("Ctrl+Shift+T opens a local terminal", () => {
    render(<Harness />);
    fireEvent.keyDown(document.body, { key: "T", ctrlKey: true, shiftKey: true });
    expect(openLocal).toHaveBeenCalledOnce();
  });

  it("Ctrl+Shift+B toggles the terminal sidebar, also inside the terminal", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
    useSettingsStore.setState({ settings: DEFAULT_SETTINGS, loaded: true });
    useUiStore.getState().setView("terminal");
    render(<Harness />);
    fireEvent.keyDown(screen.getByLabelText("Terminal-Eingabe"), { key: "B", ctrlKey: true, shiftKey: true });
    await vi.waitFor(() => expect(useSettingsStore.getState().settings.terminalSidebarOpen).toBe(true));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalSidebarOpen: true }));
  });

  it("Ctrl+Shift+B does nothing outside the terminal view", () => {
    useSettingsStore.setState({ settings: DEFAULT_SETTINGS, loaded: true });
    useUiStore.getState().setView("hosts");
    render(<Harness />);
    const event = createEvent.keyDown(document.body, { key: "B", ctrlKey: true, shiftKey: true });
    fireEvent(document.body, event);
    expect(event.defaultPrevented).toBe(false);
    expect(api.settingsSet).not.toHaveBeenCalled();
  });

  it("Ctrl+, opens the settings", () => {
    render(<Harness />);
    fireEvent.keyDown(screen.getByLabelText("Suche"), { key: ",", ctrlKey: true });
    expect(useUiStore.getState().modal).toEqual({ type: "settings" });
  });

  it("leaves Ctrl+K and Ctrl+N to the terminal but handles Ctrl+Shift+T there", () => {
    render(<Harness />);
    const terminal = screen.getByLabelText("Terminal-Eingabe");
    const event = createEvent.keyDown(terminal, { key: "k", ctrlKey: true });
    fireEvent(terminal, event);
    expect(event.defaultPrevented).toBe(false);
    fireEvent.keyDown(terminal, { key: "n", ctrlKey: true });
    expect(useUiStore.getState()).toMatchObject({ searchFocusToken: 0, modal: null });
    fireEvent.keyDown(terminal, { key: "T", ctrlKey: true, shiftKey: true });
    expect(openLocal).toHaveBeenCalledOnce();
  });

  it("is inactive while a modal is open and ignores other combinations", () => {
    render(<Harness />);
    fireEvent.keyDown(document.body, { key: "k" });
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true, altKey: true });
    expect(useUiStore.getState().searchFocusToken).toBe(0);
    useUiStore.getState().openModal({ type: "import" });
    fireEvent.keyDown(document.body, { key: ",", ctrlKey: true });
    expect(useUiStore.getState().modal).toEqual({ type: "import" });
  });

  it("maps the pane shortcuts", () => {
    const key = (k: string) => shortcutFor({ key: k, ctrlKey: true, shiftKey: true, altKey: false, metaKey: false });
    expect(key("E")).toBe("splitRow");
    expect(key("O")).toBe("splitColumn");
    expect(key("A")).toBe("toggleBroadcast");
    expect(key("W")).toBe("closeTab");
    expect(key("ArrowLeft")).toBe("focusPrev");
    expect(key("ArrowRight")).toBe("focusNext");
  });

  it("runs pane shortcuts in the terminal view only, also from inside a terminal", () => {
    const split = vi.fn();
    const toggleBroadcast = vi.fn();
    const focusNeighbour = vi.fn();
    const closeGroup = vi.fn(async () => undefined);
    useSessionsStore.setState({ activeId: "g", split, toggleBroadcast, focusNeighbour, closeGroup });
    render(<Harness />);
    const terminal = screen.getByRole("textbox", { name: "Terminal-Eingabe" });
    const press = (key: string) => fireEvent.keyDown(terminal, { key, ctrlKey: true, shiftKey: true });

    useUiStore.setState({ view: "hosts" });
    press("E");
    expect(split).not.toHaveBeenCalled();

    useUiStore.setState({ view: "terminal" });
    press("E");
    press("O");
    press("A");
    press("ArrowLeft");
    press("ArrowRight");
    press("W");
    expect(split.mock.calls).toEqual([["row"], ["column"]]);
    expect(toggleBroadcast).toHaveBeenCalledOnce();
    expect(focusNeighbour.mock.calls).toEqual([[-1], [1]]);
    expect(closeGroup).toHaveBeenCalledWith("g");
  });

  it("leaves Ctrl+Shift+←/→ to text fields for word selection", () => {
    const focusNeighbour = vi.fn();
    useSessionsStore.setState({ activeId: "g", focusNeighbour });
    useUiStore.setState({ view: "terminal" });
    render(<Harness />);
    const search = screen.getByRole("textbox", { name: "Suche" });
    const event = createEvent.keyDown(search, { key: "ArrowLeft", ctrlKey: true, shiftKey: true });
    fireEvent(search, event);
    expect(focusNeighbour).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });
});
