import { useRef } from "react";
import { Button, Dialog } from "../../components";
import { t } from "../../i18n/de";

export function CloseTabDialog({ count, onConfirm, onClose }: { count: number; onConfirm: () => void; onClose: () => void }) {
  const texts = t.sftp.closeDialog;
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <Dialog
      open
      size="sm"
      role="alertdialog"
      hideClose
      title={texts.title}
      onClose={onClose}
      initialFocusRef={cancelRef}
      footer={
        <>
          <Button ref={cancelRef} variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="danger" onClick={() => { onConfirm(); onClose(); }}>{texts.confirm}</Button>
        </>
      }
    >
      <p>{texts.text(count)}</p>
    </Dialog>
  );
}
