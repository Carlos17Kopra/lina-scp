import type { Entry } from "../../ipc/api";

export type SortKey = "name" | "size" | "mtime" | "mode" | "owner";
export interface Sort {
  key: SortKey;
  dir: "asc" | "desc";
}

const collator = new Intl.Collator("de", { numeric: true, sensitivity: "base" });

export function isDirLike(entry: Entry): boolean {
  return entry.kind.type === "dir" || (entry.kind.type === "symlink" && entry.kind.target === "dir");
}

export function isHidden(entry: Entry): boolean {
  return entry.name.startsWith(".");
}

function compare(a: Entry, b: Entry, key: SortKey): number {
  switch (key) {
    case "name":
      return collator.compare(a.name, b.name);
    case "size":
      return a.size - b.size;
    case "mtime":
      return (a.mtime ?? 0) - (b.mtime ?? 0);
    case "mode":
      return (a.mode ?? 0) - (b.mode ?? 0);
    case "owner":
      return collator.compare(a.owner ?? "", b.owner ?? "");
  }
}

/** Folders (and links to folders) first; then by key, ties by name. */
export function sortEntries(entries: readonly Entry[], sort: Sort): Entry[] {
  const sign = sort.dir === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    const dirs = Number(isDirLike(b)) - Number(isDirLike(a));
    if (dirs !== 0) return dirs;
    return sign * (compare(a, b, sort.key) || collator.compare(a.name, b.name));
  });
}

export function visibleEntries(entries: readonly Entry[], showHidden: boolean, sort: Sort): Entry[] {
  return sortEntries(showHidden ? entries : entries.filter((entry) => !isHidden(entry)), sort);
}

const number1 = new Intl.NumberFormat("de", { maximumFractionDigits: 1 });
const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatSize(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${unit === 0 ? value : number1.format(value)} ${UNITS[unit]}`;
}

const pad = (n: number) => String(n).padStart(2, "0");

export function formatMtime(seconds: number | null): string {
  if (seconds === null) return "";
  const d = new Date(seconds * 1000);
  return `${pad(d.getDate())}.${pad(d.getMonth() + 1)}.${d.getFullYear()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatMode(mode: number | null): string {
  if (mode === null) return "";
  const triple = (bits: number, special: boolean, specialChar: string) =>
    `${bits & 4 ? "r" : "-"}${bits & 2 ? "w" : "-"}${special ? (bits & 1 ? specialChar : specialChar.toUpperCase()) : bits & 1 ? "x" : "-"}`;
  return (
    triple((mode >> 6) & 7, (mode & 0o4000) !== 0, "s") +
    triple((mode >> 3) & 7, (mode & 0o2000) !== 0, "s") +
    triple(mode & 7, (mode & 0o1000) !== 0, "t")
  );
}

export function parentPath(path: string): string {
  const cut = path.replace(/\/+$/, "").lastIndexOf("/");
  return cut <= 0 ? "/" : path.slice(0, cut);
}

export function joinPath(dir: string, name: string): string {
  return `${dir.replace(/\/+$/, "")}/${name}`;
}

export function crumbs(path: string): { label: string; path: string }[] {
  const parts = path.split("/").filter(Boolean);
  return [{ label: "/", path: "/" }, ...parts.map((label, i) => ({ label, path: `/${parts.slice(0, i + 1).join("/")}` }))];
}
