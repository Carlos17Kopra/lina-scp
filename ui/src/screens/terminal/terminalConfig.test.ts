import { describe, expect, it } from "vitest";
import { clipboardShortcut, shouldUseWebgl } from "./terminalConfig";
import { FONT_SIZE_MAX, FONT_SIZE_MIN, isKnownScheme, SCHEME_CHOICES, TERMINAL_FONT_FAMILY, terminalTheme } from "./theme";

const key = (key: string, mods: Partial<KeyboardEvent> = {}) =>
  ({ key, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods }) as KeyboardEvent;

describe("terminal theme", () => {
  it("maps every scheme to an opaque xterm theme; auto and unknown ids follow the app theme", () => {
    expect(terminalTheme("auto", "dark")).toMatchObject({ background: "#0B0E13", foreground: "#D4DAE2", cursor: "#4FD1C5", green: "#3DDC97" });
    expect(terminalTheme("auto", "light")).toMatchObject({ background: "#FFFFFF", foreground: "#2A313B", cursor: "#0E7C72" });
    expect(terminalTheme("solarizedNeon", "light")).toEqual(terminalTheme("auto", "light"));
    expect(terminalTheme("nord", "light")).toMatchObject({ background: "#2E3440", brightBlack: "#7B88A1", black: "#3B4252" });
    for (const { id } of SCHEME_CHOICES) {
      expect(terminalTheme(id, "dark").background).toMatch(/^#[0-9A-F]{6}$/);
    }
    expect(SCHEME_CHOICES.map((choice) => choice.id)).toEqual([
      "auto", "linascp", "linascpLight", "dracula", "nord", "gruvbox", "tokyo", "solDark", "solLight", "contrast",
    ]);
    expect(SCHEME_CHOICES[0]?.label).toBe("Wie App-Design");
    expect(isKnownScheme("nord")).toBe(true);
    expect(isKnownScheme("solarizedNeon")).toBe(false);
    expect(isKnownScheme("toString")).toBe(false);
    expect([FONT_SIZE_MIN, FONT_SIZE_MAX]).toEqual([10, 20]);
    expect(TERMINAL_FONT_FAMILY).toContain("JetBrains Mono");
  });
});

describe("shouldUseWebgl", () => {
  it("uses WebGL only on visible terminals that have not failed and are not set to DOM", () => {
    expect(shouldUseWebgl("auto", true, false)).toBe(true);
    expect(shouldUseWebgl("webgl", true, false)).toBe(true);
    expect(shouldUseWebgl("auto", false, false)).toBe(false);
    expect(shouldUseWebgl("auto", true, true)).toBe(false);
    expect(shouldUseWebgl("dom", true, false)).toBe(false);
  });
});

describe("clipboardShortcut", () => {
  it("detects Ctrl+Shift+C/V only", () => {
    expect(clipboardShortcut(key("C", { ctrlKey: true, shiftKey: true }))).toBe("copy");
    expect(clipboardShortcut(key("v", { ctrlKey: true, shiftKey: true }))).toBe("paste");
    expect(clipboardShortcut(key("c", { ctrlKey: true }))).toBeNull();
    expect(clipboardShortcut(key("v", { ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
    expect(clipboardShortcut(key("t", { ctrlKey: true, shiftKey: true }))).toBeNull();
  });
});
