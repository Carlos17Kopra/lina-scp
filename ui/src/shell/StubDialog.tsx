import { Dialog } from "../components";
import { t } from "../i18n/de";
import { useUiStore } from "../stores/ui";

/** Temporary modal body until the real dialog of a later task replaces the stub component. */
export function StubDialog({ title }: { title: string }) {
  const closeModal = useUiStore((state) => state.closeModal);
  return (
    <Dialog open title={title} onClose={closeModal} size="sm">
      <p>{t.common.comingSoon}</p>
    </Dialog>
  );
}
