import type { ITheme } from "@xterm/xterm";
import type { Theme } from "../../ipc/api";
import schemes from "./schemes.json";

interface Scheme {
  name: string;
  bg: string;
  fg: string;
  dim: string;
  green: string;
  blue: string;
  yellow: string;
  red: string;
  cursor: string;
  bd: string;
}

/** Scheme "auto" (default until TP2 adds the scheme picker): dark → `linascp`, light → `linascpLight`. */
const DARK: Scheme = schemes.schemes.linascp;
const LIGHT: Scheme = schemes.schemes.linascpLight;

export const TERMINAL_FONT_FAMILY = '"JetBrains Mono", monospace';
export const FONT_SIZE_MIN: number = schemes.fontSize.min;
export const FONT_SIZE_MAX: number = schemes.fontSize.max;

/** xterm theme with an opaque background (transparency causes artifacts in WebKitGTK). */
export function terminalTheme(appTheme: Theme): ITheme {
  const s = appTheme === "light" ? LIGHT : DARK;
  return {
    background: s.bg,
    foreground: s.fg,
    cursor: s.cursor,
    cursorAccent: s.bg,
    selectionBackground: `${s.cursor}55`,
    black: s.bd,
    brightBlack: s.dim,
    red: s.red,
    brightRed: s.red,
    green: s.green,
    brightGreen: s.green,
    yellow: s.yellow,
    brightYellow: s.yellow,
    blue: s.blue,
    brightBlue: s.blue,
    white: s.fg,
    brightWhite: s.fg,
  };
}
