/** `{{name}}` with a 1–32 character name; anything else stays literal text. */
const PLACEHOLDER = /\{\{([A-Za-z0-9_-]{1,32})\}\}/g;

/** Unique placeholder names in order of first use. */
export function placeholderNames(text: string): string[] {
  const names: string[] = [];
  for (const match of text.matchAll(PLACEHOLDER)) {
    const name = match[1] as string;
    if (!names.includes(name)) names.push(name);
  }
  return names;
}

/** Replaces every placeholder with its value, verbatim (no quoting, no re-resolution). */
export function fillPlaceholders(text: string, values: Record<string, string>): string {
  return text.replace(PLACEHOLDER, (_match, name: string) => values[name] ?? "");
}
