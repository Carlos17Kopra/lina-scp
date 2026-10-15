import type { Snippet } from "../../ipc/api";

export function firstLine(text: string): string {
  return text.split("\n", 1)[0] ?? "";
}

/** Case-insensitive match on name and command; an empty query matches everything. */
export function matchesQuery(snippet: Snippet, query: string): boolean {
  const needle = query.trim().toLowerCase();
  return !needle || snippet.name.toLowerCase().includes(needle) || snippet.command.toLowerCase().includes(needle);
}
