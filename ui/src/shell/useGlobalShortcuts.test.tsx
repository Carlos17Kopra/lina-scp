import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

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
});
