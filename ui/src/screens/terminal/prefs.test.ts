import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../stores/hosts";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { makeHost } from "../../test/fixtures";
import { effectivePrefs, useTerminalPrefs } from "./prefs";

vi.mock("../../ipc/api");

const settings = { ...DEFAULT_SETTINGS, terminalScheme: "dracula", terminalFontSize: 13 };

beforeEach(() => {
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState({ settings, loaded: true });
});

describe("effectivePrefs", () => {
  it("prefers the host override per field and falls back to the global settings", () => {
    expect(effectivePrefs(undefined, settings)).toEqual({ scheme: "dracula", fontSize: 13 });
    expect(effectivePrefs(makeHost({ terminalScheme: "nord", terminalFontSize: null }), settings)).toEqual({ scheme: "nord", fontSize: 13 });
    expect(effectivePrefs(makeHost({ terminalScheme: null, terminalFontSize: 18 }), settings)).toEqual({ scheme: "dracula", fontSize: 18 });
  });

  it("uses the global values for local tabs and deleted hosts", () => {
    useHostsStore.setState({ hosts: [makeHost({ id: "h1", terminalScheme: "nord", terminalFontSize: 18 })] });
    expect(renderHook(() => useTerminalPrefs("h1")).result.current).toEqual({ scheme: "nord", fontSize: 18 });
    expect(renderHook(() => useTerminalPrefs(undefined)).result.current).toEqual({ scheme: "dracula", fontSize: 13 });
    expect(renderHook(() => useTerminalPrefs("gone")).result.current).toEqual({ scheme: "dracula", fontSize: 13 });
  });
});
