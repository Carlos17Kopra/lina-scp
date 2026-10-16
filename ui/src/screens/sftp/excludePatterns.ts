/** Exclude patterns of sync and keep-up-to-date, remembered per host. */

const excludeKey = (hostId: string) => `linascp.sync.exclude.${hostId}`;

export function loadExclude(hostId: string): string {
  try {
    return localStorage.getItem(excludeKey(hostId)) ?? "";
  } catch {
    return "";
  }
}

export function saveExclude(hostId: string, text: string) {
  try {
    localStorage.setItem(excludeKey(hostId), text);
  } catch {
    // ponytail: a remembered filter is a convenience; without storage it is simply not remembered.
  }
}

export const patterns = (text: string) => text.split(",").map((p) => p.trim()).filter(Boolean);
