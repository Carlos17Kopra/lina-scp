import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { dropStream, registerStream } from "../../stores/streams";
import { useUiStore } from "../../stores/ui";
import { makeHost, singleGroups } from "../../test/fixtures";
import { getPasteTarget } from "./pasteTargets";
import { TerminalView } from "./TerminalView";

const fakes = vi.hoisted(() => {
  class Emitter<T> {
    listeners: Array<(value: T) => void> = [];
    event = (listener: (value: T) => void) => {
      this.listeners.push(listener);
      return { dispose: () => (this.listeners = this.listeners.filter((l) => l !== listener)) };
    };
    fire(value: T) {
      for (const listener of this.listeners) listener(value);
    }
  }
  class FakeTerminal {
    static instances: FakeTerminal[] = [];
    options: Record<string, unknown>;
    cols = 80;
    rows = 24;
    unicode = { activeVersion: "6" };
    element: HTMLElement | null = null;
    written: Array<string | Uint8Array> = [];
    selection = "";
    pasted: string[] = [];
    inputs: string[] = [];
    keyHandler: ((event: KeyboardEvent) => boolean) | null = null;
    disposed = false;
    data = new Emitter<string>();
    binary = new Emitter<string>();
    resize = new Emitter<{ cols: number; rows: number }>();
    selectionChange = new Emitter<void>();
    onData = this.data.event;
    onBinary = this.binary.event;
    onResize = this.resize.event;
    onSelectionChange = this.selectionChange.event;
    constructor(options: Record<string, unknown>) {
      this.options = { ...options };
      FakeTerminal.instances.push(this);
    }
    loadAddon(addon: { activate(terminal: unknown): void }) {
      addon.activate(this);
    }
    open(element: HTMLElement) {
      this.element = element;
    }
    write(data: string | Uint8Array, callback?: () => void) {
      this.written.push(data);
      callback?.();
    }
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      this.keyHandler = handler;
    }
    hasSelection() {
      return this.selection !== "";
    }
    getSelection() {
      return this.selection;
    }
    paste(text: string) {
      this.pasted.push(text);
    }
    input(data: string) {
      this.inputs.push(data);
    }
    focused = 0;
    focus() {
      this.focused += 1;
    }
    dispose() {
      this.disposed = true;
    }
  }
  class FakeWebgl {
    static instances: FakeWebgl[] = [];
    static failNext = false;
    disposed = false;
    contextLoss = new Emitter<void>();
    onContextLoss = this.contextLoss.event;
    constructor() {
      if (FakeWebgl.failNext) {
        FakeWebgl.failNext = false;
        throw new Error("WebGL2 not supported");
      }
      FakeWebgl.instances.push(this);
    }
    activate() {}
    dispose() {
      this.disposed = true;
    }
  }
  class FakeFit {
    static fits = 0;
    activate() {}
    dispose() {}
    fit() {
      FakeFit.fits += 1;
    }
  }
  class FakeAddon {
    activate() {}
    dispose() {}
  }
  return { FakeTerminal, FakeWebgl, FakeFit, FakeAddon };
});

vi.mock("@xterm/xterm", () => ({ Terminal: fakes.FakeTerminal }));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: fakes.FakeWebgl }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: fakes.FakeFit }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: fakes.FakeAddon }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: fakes.FakeAddon }));
vi.mock("../../ipc/api");
const clipboard = vi.hoisted(() => ({
  writeText: vi.fn(async (_text: string) => undefined),
  readText: vi.fn(async () => "pasted"),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => clipboard);

class FakeResizeObserver {
  observe() {}
  disconnect() {}
}

const tab: Tab = { id: "tab-1", title: "web-prod-01", kind: "ssh", hostId: "h", sessionId: "s-1", status: "connected" };
const detach = vi.fn();
const stream = {
  channel: {},
  bind: vi.fn(),
  ack: vi.fn(),
  attach: vi.fn((onData: (bytes: Uint8Array) => void) => {
    onData(new Uint8Array([104, 105]));
    return detach;
  }),
};
// The webview clipboard cannot read other apps' content on WebKitGTK; it must stay unused.
const webviewClipboard = { writeText: vi.fn(async () => undefined), readText: vi.fn(async () => "webview") };
const term = () => fakes.FakeTerminal.instances[0] as InstanceType<typeof fakes.FakeTerminal>;
const flush = () => act(async () => undefined);

beforeEach(() => {
  fakes.FakeTerminal.instances = [];
  fakes.FakeWebgl.instances = [];
  fakes.FakeWebgl.failNext = false;
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  Object.defineProperty(navigator, "clipboard", { value: webviewClipboard, configurable: true });
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  registerStream(tab.id, stream as unknown as api.OutputStream);
});

afterEach(() => {
  dropStream(tab.id);
  vi.unstubAllGlobals();
});

describe("TerminalView", () => {
  it("creates an opaque xterm attached to the session output with flow-control acks", () => {
    const { container } = render(<TerminalView tab={tab} visible />);
    expect(container.querySelector("[data-terminal]")).not.toBeNull();
    expect(term().options).toMatchObject({
      fontSize: 14,
      fontFamily: '"JetBrains Mono", monospace',
      allowProposedApi: true,
      allowTransparency: false,
      theme: { background: "#0B0E13" },
    });
    expect(term().unicode.activeVersion).toBe("11");
    expect(term().written).toEqual([new Uint8Array([104, 105])]);
    expect(stream.ack).toHaveBeenCalledWith(2);
    expect(fakes.FakeWebgl.instances).toHaveLength(1);
  });

  it("reports the terminal size to the tab", () => {
    useSessionsStore.setState({ tabs: [tab], activeId: tab.id });
    render(<TerminalView tab={tab} visible />);
    expect(useSessionsStore.getState().tabs[0]).toMatchObject({ cols: 80, rows: 24 });
    act(() => term().resize.fire({ cols: 120, rows: 40 }));
    expect(useSessionsStore.getState().tabs[0]).toMatchObject({ cols: 120, rows: 40 });
  });

  it("sends input and resizes to the session", () => {
    render(<TerminalView tab={tab} visible />);
    expect(api.sessionResize).toHaveBeenCalledWith("s-1", 80, 24);
    term().data.fire("ls\r");
    expect(api.sessionWrite).toHaveBeenCalledWith("s-1", "ls\r");
    term().binary.fire("\u0080");
    expect(api.sessionWrite).toHaveBeenLastCalledWith("s-1", new Uint8Array([0x80]));
    term().resize.fire({ cols: 100, rows: 30 });
    expect(api.sessionResize).toHaveBeenCalledWith("s-1", 100, 30);
  });

  it("copies selections and pastes via Ctrl+Shift+V and middle click", async () => {
    render(<TerminalView tab={tab} visible />);
    term().selection = "uptime";
    term().selectionChange.fire();
    expect(clipboard.writeText).toHaveBeenCalledWith("uptime");
    expect(webviewClipboard.writeText).not.toHaveBeenCalled();
    const handler = term().keyHandler as (event: KeyboardEvent) => boolean;
    expect(handler(new KeyboardEvent("keydown", { key: "V", ctrlKey: true, shiftKey: true }))).toBe(false);
    await flush();
    expect(term().pasted).toEqual(["pasted"]);
    expect(handler(new KeyboardEvent("keydown", { key: "C", ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(clipboard.writeText).toHaveBeenCalledTimes(2);
    expect(handler(new KeyboardEvent("keydown", { key: "c", ctrlKey: true }))).toBe(true);
    fireEvent.mouseUp(term().element as HTMLElement, { button: 1 });
    await flush();
    expect(term().pasted).toEqual(["pasted", "pasted"]);
    expect(webviewClipboard.readText).not.toHaveBeenCalled();
  });

  it("pastes nothing when the system clipboard cannot be read", async () => {
    clipboard.readText.mockRejectedValueOnce(new Error("empty clipboard"));
    render(<TerminalView tab={tab} visible />);
    const handler = term().keyHandler as (event: KeyboardEvent) => boolean;
    handler(new KeyboardEvent("keydown", { key: "V", ctrlKey: true, shiftKey: true }));
    await flush();
    expect(term().pasted).toEqual([]);
  });

  it("keeps WebGL only while visible and falls back to the DOM renderer on context loss", () => {
    const { rerender } = render(<TerminalView tab={tab} visible />);
    const first = fakes.FakeWebgl.instances[0];
    rerender(<TerminalView tab={tab} visible={false} />);
    expect(first?.disposed).toBe(true);
    rerender(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(2);
    act(() => fakes.FakeWebgl.instances[1]?.contextLoss.fire());
    expect(fakes.FakeWebgl.instances[1]?.disposed).toBe(true);
    rerender(<TerminalView tab={tab} visible={false} />);
    rerender(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(2);
  });

  it("never loads WebGL with the DOM renderer and survives WebGL failures", () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalRenderer: "dom" } });
    const { unmount } = render(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(0);
    unmount();
    useSettingsStore.setState({ settings: DEFAULT_SETTINGS });
    fakes.FakeWebgl.failNext = true;
    render(<TerminalView tab={tab} visible />);
    expect(fakes.FakeWebgl.instances).toHaveLength(0);
  });

  it("applies font size and theme changes", () => {
    render(<TerminalView tab={tab} visible />);
    act(() => useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalFontSize: 16 } }));
    expect(term().options.fontSize).toBe(16);
    act(() => useUiStore.getState().setTheme("light"));
    expect(term().options.theme).toMatchObject({ background: "#FFFFFF" });
  });

  it("applies the host's scheme and font size override and follows changes", () => {
    useHostsStore.setState({ hosts: [makeHost({ id: "h", terminalScheme: "nord", terminalFontSize: 16 })] });
    const { container } = render(<TerminalView tab={tab} visible />);
    expect(term().options).toMatchObject({ fontSize: 16, theme: { background: "#2E3440" } });
    expect((container.querySelector("[data-terminal]") as HTMLElement).style.background).toBe("rgb(46, 52, 64)");
    act(() => useHostsStore.setState({ hosts: [makeHost({ id: "h", terminalScheme: null, terminalFontSize: null })] }));
    expect(term().options).toMatchObject({ fontSize: 14, theme: { background: "#0B0E13" } });
  });

  it("registers a paste target: insert pastes, run pastes then sends Enter through xterm input", () => {
    const { unmount } = render(<TerminalView tab={tab} visible />);
    const target = getPasteTarget(tab.id);
    target?.insert("ls -la");
    target?.run("cd /srv\ngit pull");
    expect(term().pasted).toEqual(["ls -la", "cd /srv\ngit pull"]);
    // Enter goes through term.input → onData → the same ordered sessionWrite queue as the paste.
    expect(term().inputs).toEqual(["\r"]);
    unmount();
    expect(getPasteTarget(tab.id)).toBeUndefined();
  });

it("broadcasts input of the focused pane to connected peers of its group (Review Focus 4)", () => {
    const peer: Tab = { id: "tab-2", title: "db", kind: "ssh", sessionId: "s-2", status: "connected" };
    const picking: Tab = { id: "tab-3", title: "Neuer Bereich", kind: "ssh", status: "picking" };
    useSessionsStore.setState({
      tabs: [tab, peer, picking],
      groups: [
        {
          id: tab.id,
          layout: {
            type: "split",
            dir: "row",
            ratio: 0.5,
            a: { type: "pane", paneId: tab.id },
            b: { type: "split", dir: "column", ratio: 0.5, a: { type: "pane", paneId: "tab-2" }, b: { type: "pane", paneId: "tab-3" } },
          },
          focusedPaneId: tab.id,
          broadcast: true,
        },
      ],
      activeId: tab.id,
    });
    render(<TerminalView tab={tab} visible />);
    term().data.fire("uptime\r");
    expect(api.sessionWrite).toHaveBeenCalledWith("s-1", "uptime\r");
    expect(api.sessionWrite).toHaveBeenCalledWith("s-2", "uptime\r");
    expect(api.sessionWrite).toHaveBeenCalledTimes(2);
    term().binary.fire("\u0080");
    expect(api.sessionWrite).toHaveBeenLastCalledWith("s-2", new Uint8Array([0x80]));
    // Unfocused: only its own session.
    vi.mocked(api.sessionWrite).mockClear();
    act(() => useSessionsStore.getState().focusPane("tab-2"));
    term().data.fire("x");
    expect(vi.mocked(api.sessionWrite).mock.calls).toEqual([["s-1", "x"]]);
  });

  it("does not broadcast without the flag", () => {
    useSessionsStore.setState({ tabs: [tab], groups: singleGroups([tab]), activeId: tab.id });
    render(<TerminalView tab={tab} visible />);
    term().data.fire("ls");
    expect(vi.mocked(api.sessionWrite).mock.calls).toEqual([["s-1", "ls"]]);
  });

  it("takes DOM focus when it becomes the focused pane", () => {
    const { rerender } = render(<TerminalView tab={tab} visible focused={false} />);
    const before = term().focused;
    rerender(<TerminalView tab={tab} visible focused />);
    expect(term().focused).toBe(before + 1);
  });

  it("detaches and disposes on unmount", () => {
    const { unmount } = render(<TerminalView tab={tab} visible />);
    unmount();
    expect(detach).toHaveBeenCalled();
    expect(term().disposed).toBe(true);
    expect(fakes.FakeWebgl.instances[0]?.disposed).toBe(true);
  });

  it("does not grab the keyboard when an unfocused pane becomes visible (review I1)", () => {
    const { rerender } = render(<TerminalView tab={tab} visible={false} />);
    rerender(<TerminalView tab={tab} visible />);
    expect(term().focused).toBe(0);
  });

  it("does not broadcast automatic terminal replies, only typed input (review I3)", () => {
    const peer: Tab = { id: "tab-2", title: "db", kind: "ssh", sessionId: "s-2", status: "connected" };
    useSessionsStore.setState({
      tabs: [tab, peer],
      groups: [
        {
          id: tab.id,
          layout: { type: "split", dir: "row", ratio: 0.5, a: { type: "pane", paneId: tab.id }, b: { type: "pane", paneId: "tab-2" } },
          focusedPaneId: tab.id,
          broadcast: true,
        },
      ],
      activeId: tab.id,
    });
    render(<TerminalView tab={tab} visible />);
    for (const reply of ["\u001b[?1;2c", "\u001b[12;5R", "\u001b[0n", "\u001b[?2004;2$y", "\u001b[I", "\u001b[O", "\u001b]11;rgb:0000/0000/0000\u0007"]) {
      term().data.fire(reply);
    }
    expect(vi.mocked(api.sessionWrite).mock.calls.every(([session]) => session === "s-1")).toBe(true);
    term().data.fire("\u001b[A");
    expect(api.sessionWrite).toHaveBeenLastCalledWith("s-2", "\u001b[A");
  });
});
