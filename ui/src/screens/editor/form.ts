import type { HostDraft } from "./drafts";
import type { HostField } from "./validate";

/** Props shared by the host editor tabs. */
export interface HostFormProps {
  draft: HostDraft;
  /** Applies a patch; `field` marks the field as touched so its error becomes visible. */
  edit(patch: Partial<HostDraft>, field?: HostField): void;
  /** Error of a touched field. */
  fieldError(field: HostField): string | undefined;
}
