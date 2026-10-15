import type { CoreError } from "./bindings";

const CORE_ERROR_TYPES: ReadonlySet<string> = new Set<CoreError["type"]>([
  "network",
  "hostKeyRejected",
  "auth",
  "keyring",
  "store",
  "config",
  "io",
  "fs",
  "cancelled",
  "inputRequired",
  "internal",
]);

/** Rejections of `ipc/api.ts` calls are `CoreError`s (tagged by `type`). */
export function isCoreError(value: unknown): value is CoreError {
  if (typeof value !== "object" || value === null) return false;
  const type = (value as { type?: unknown }).type;
  return typeof type === "string" && CORE_ERROR_TYPES.has(type);
}

/** Short message for UI text (backend messages are not translated). */
export function errorMessage(error: unknown): string {
  if (isCoreError(error)) return "message" in error ? error.message : error.type;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Technical details for the "Details kopieren" toast action. */
export function errorDetails(error: unknown): string {
  if (isCoreError(error)) return JSON.stringify(error, null, 2);
  if (error instanceof Error) return error.stack ?? error.message;
  return String(error);
}
