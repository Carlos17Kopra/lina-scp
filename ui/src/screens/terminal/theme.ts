import type { ITheme } from "@xterm/xterm";
import { t } from "../../i18n/de";
import type { Theme } from "../../ipc/api";
import schemes from "./schemes.json";

export interface Scheme {
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

const SCHEMES: Readonly<Record<string, Scheme>> = schemes.schemes;

/** "Wie App-Design": dark → `linascp`, light → `linascpLight`. */
export const AUTO_SCHEME = "auto";

/** Picker entries: "Wie App-Design" first, then the schemes in `schemes.json` order. */
export const SCHEME_CHOICES: ReadonlyArray<{ id: string; label: string }> = [
  { id: AUTO_SCHEME, label: t.terminal.design.auto },
  ...Object.entries(SCHEMES).map(([id, scheme]) => ({ id, label: scheme.name })),
];

export function isKnownScheme(id: string): boolean {
  return Object.hasOwn(SCHEMES, id);
}

/** Unknown ids (e.g. stored by a newer build) behave like "auto". */
export function resolveScheme(schemeId: string, appTheme: Theme): Scheme {
  const own = isKnownScheme(schemeId) ? SCHEMES[schemeId] : undefined;
  return own ?? (appTheme === "light" ? schemes.schemes.linascpLight : schemes.schemes.linascp);
}

export const TERMINAL_FONT_FAMILY = '"JetBrains Mono", monospace';
export const FONT_SIZE_MIN: number = schemes.fontSize.min;
export const FONT_SIZE_MAX: number = schemes.fontSize.max;

/** xterm theme with an opaque background (transparency causes artifacts in WebKitGTK). */
// ponytail: schemes define 9 colors; magenta/cyan stay xterm defaults, bright = normal.
export function terminalTheme(schemeId: string, appTheme: Theme): ITheme {
  const s = resolveScheme(schemeId, appTheme);
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
