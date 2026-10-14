import { t } from "../../i18n/de";
import type { Id, ImportPreview, ImportReport, Resolution } from "../../ipc/api";
import { isCoreError } from "../../ipc/errors";

export const LINASCP_EXTENSION = "linascp";
export const LINASCP_FILTER = { name: t.settings.export.filter, extensions: [LINASCP_EXTENSION] };
export const MIN_PASSPHRASE_LENGTH = 8;

const pad = (value: number) => String(value).padStart(2, "0");

export function exportFileName(date: Date = new Date()): string {
  return `linascp-${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}.${LINASCP_EXTENSION}`;
}

/** The GTK save dialog does not append the filter's extension. */
export function ensureExtension(path: string): string {
  return path.endsWith(`.${LINASCP_EXTENSION}`) ? path : `${path}.${LINASCP_EXTENSION}`;
}

export interface ExportForm {
  includeSecrets: boolean;
  passphrase: string;
  repeat: string;
}

export function validateExport(form: ExportForm): { passphrase?: string; repeat?: string } {
  if (!form.includeSecrets) return {};
  if (form.passphrase.length < MIN_PASSPHRASE_LENGTH) return { passphrase: t.settings.export.tooShort };
  if (form.repeat !== form.passphrase) return { repeat: t.settings.export.mismatch };
  return {};
}

/** Every conflicting item starts as "Überspringen"; items without conflict are imported as-is. */
export function defaultResolutions(preview: ImportPreview): Record<Id, Resolution> {
  return Object.fromEntries(preview.items.filter((item) => item.conflict !== null).map((item) => [item.id, "skip" as const]));
}

export function reportMessage(report: ImportReport): string {
  return t.settings.import.report(report.imported, report.skipped, report.secretsRestored);
}

/** Core's import rejects a wrong passphrase with `Config { message: "wrong passphrase" }`. */
export function isWrongPassphrase(error: unknown): boolean {
  return isCoreError(error) && error.type === "config" && error.message === "wrong passphrase";
}
