import { t } from "../../i18n/de";
import { type AuthDraft, type EditorTab, type HostDraft, NEW_GROUP, type ProfileDraft } from "./drafts";

export type HostField =
  | "name"
  | "address"
  | "user"
  | "port"
  | "newGroupName"
  | "profileId"
  | "keyPath"
  | "keepalive"
  | "timeout"
  | "env"
  | "forwards"
  | "parallel"
  | "bandwidthLimit";
export type ProfileField = "name" | "user" | "keyPath";
export type FieldErrors<F extends string> = Partial<Record<F, string>>;

export const HOST_FIELD_TAB: Record<HostField, EditorTab> = {
  name: "general",
  address: "general",
  user: "auth",
  port: "general",
  newGroupName: "general",
  profileId: "auth",
  keyPath: "auth",
  keepalive: "connection",
  timeout: "connection",
  env: "connection",
  forwards: "connection",
  parallel: "sftp",
  bandwidthLimit: "sftp",
};

const TAB_ORDER: readonly EditorTab[] = ["general", "auth", "connection", "sftp"];
const INT_RE = /^\d+$/;
const ENV_LINE_RE = /^[A-Za-z_][A-Za-z0-9_]*=/;

export function isNonNegativeInt(text: string): boolean {
  return INT_RE.test(text.trim());
}

export function validatePort(text: string): string | undefined {
  const value = text.trim();
  if (!INT_RE.test(value)) return t.editor.errors.port;
  const port = Number(value);
  return port >= 1 && port <= 65535 ? undefined : t.editor.errors.port;
}

/** First invalid non-empty line (1-based) of the env text area. */
export function envError(text: string): string | undefined {
  for (const [index, raw] of text.split("\n").entries()) {
    const line = raw.trim();
    if (line && !ENV_LINE_RE.test(line)) return t.editor.errors.env(index + 1);
  }
  return undefined;
}

/** Mirrors core `is_safe_value`: OpenSSH misparses `'`, `\\` and a leading `#` in config values. */
export function isSafeValue(text: string): boolean {
  return !/['\\]/.test(text) && !text.trimStart().startsWith("#");
}

function keyPathError(auth: AuthDraft): string | undefined {
  if (auth.method !== "key") return undefined;
  const path = auth.keyPath.trim();
  if (!path) return t.editor.errors.keyPath;
  return isSafeValue(path) ? undefined : t.editor.errors.unsafeValue("Schlüsseldatei");
}

export function validateHost(draft: HostDraft): FieldErrors<HostField> {
  const errors: FieldErrors<HostField> = {};
  const nonNegative = t.editor.errors.nonNegative;
  if (!draft.name.trim()) errors.name = t.editor.errors.required;
  const address = draft.address.trim();
  if (!address) errors.address = t.editor.errors.required;
  else if (/\s/.test(address)) errors.address = t.editor.errors.address;
  else if (!isSafeValue(address)) errors.address = t.editor.errors.unsafeValue("Adresse");
  const port = validatePort(draft.port);
  if (port) errors.port = port;
  if (draft.groupId === NEW_GROUP && !draft.newGroupName.trim()) errors.newGroupName = t.editor.errors.groupName;
  if (draft.authSource === "profile" && !draft.profileId) errors.profileId = t.editor.errors.profile;
  if (draft.authSource === "custom") {
    const keyPath = keyPathError(draft.auth);
    if (keyPath) errors.keyPath = keyPath;
    if (!isSafeValue(draft.user.trim())) errors.user = t.editor.errors.unsafeValue(t.editor.user);
  }
  if (!isNonNegativeInt(draft.keepalive)) errors.keepalive = nonNegative;
  if (!isNonNegativeInt(draft.timeout)) errors.timeout = nonNegative;
  const env = envError(draft.env);
  if (env) errors.env = env;
  if (draft.forwards.some((f) => !f.bind.trim() || (f.kind !== "dynamic" && !f.target.trim()))) {
    errors.forwards = t.editor.errors.forward;
  }
  if (!isNonNegativeInt(draft.sftp.parallel)) errors.parallel = nonNegative;
  if (draft.sftp.bandwidthLimit.trim() && !isNonNegativeInt(draft.sftp.bandwidthLimit)) errors.bandwidthLimit = nonNegative;
  return errors;
}

export function validateProfile(draft: ProfileDraft): FieldErrors<ProfileField> {
  const errors: FieldErrors<ProfileField> = {};
  if (!draft.name.trim()) errors.name = t.editor.errors.required;
  if (!draft.user.trim()) errors.user = t.editor.errors.required;
  else if (!isSafeValue(draft.user.trim())) errors.user = t.editor.errors.unsafeValue(t.editor.user);
  const keyPath = keyPathError(draft.auth);
  if (keyPath) errors.keyPath = keyPath;
  return errors;
}

export function hasErrors(errors: object): boolean {
  return Object.keys(errors).length > 0;
}

/** Tabs that contain at least one error, in tab order. */
export function errorTabs(errors: FieldErrors<HostField>): EditorTab[] {
  const tabs = new Set((Object.keys(errors) as HostField[]).map((field) => HOST_FIELD_TAB[field]));
  return TAB_ORDER.filter((tab) => tabs.has(tab));
}
