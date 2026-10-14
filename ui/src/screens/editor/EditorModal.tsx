import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";
import type { EditorTarget } from "../../stores/ui";

/** Stub — replaced by the host/profile editor in Task 20 (same props). */
export function EditorModal({ target }: { target: EditorTarget }) {
  return <StubDialog title={target.kind === "host" ? t.stubs.editorHost : t.stubs.editorProfile} />;
}
