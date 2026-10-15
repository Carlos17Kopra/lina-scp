import { useRef } from "react";
import { Button, Dialog } from "../../components";
import { t } from "../../i18n/de";

export function DeleteDialog({ mode, names, folder, onConfirm, onClose }: {
  mode: "trash" | "permanent";
  names: string[];
  folder: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const texts = t.sftp.deleteDialog;
  const cancelRef = useRef<HTMLButtonElement>(null);
  const trash = mode === "trash";
  const one = names.length === 1 ? names[0]! : null;
  const text = trash
    ? one ? texts.trashOne(one) : texts.trashMany(names.length)
    : one ? texts.one(one) : texts.many(names.length);
  return (
    <Dialog
      open
      size="sm"
      role="alertdialog"
      title={trash ? texts.trashTitle : texts.permanentTitle}
      onClose={onClose}
      initialFocusRef={cancelRef}
      footer={
        <>
          <Button ref={cancelRef} variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant={trash ? "primary" : "danger"} onClick={() => { onConfirm(); onClose(); }}>
            {trash ? texts.confirmTrash : texts.confirm}
          </Button>
        </>
      }
    >
      <p>{text}</p>
      {folder ? <p>{texts.folderHint}</p> : null}
    </Dialog>
  );
}
