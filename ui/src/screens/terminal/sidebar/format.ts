import { t } from "../../../i18n/de";

/** "14:02 (2 Std. 14 Min.)": local clock time of `since` plus the elapsed time until `now`. */
export function formatSince(since: number, now: number): string {
  const clock = new Date(since).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  const minutes = Math.max(0, Math.floor((now - since) / 60_000));
  return `${clock} (${t.terminal.info.duration(Math.floor(minutes / 60), minutes % 60)})`;
}
