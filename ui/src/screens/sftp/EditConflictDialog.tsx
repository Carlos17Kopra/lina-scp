import { useRef } from "react";
import { Button, Dialog } from "../../components";
import { t } from "../../i18n/de";
import { conflictOf, useEditsStore } from "../../stores/edits";
import styles from "./SftpScreen.module.css";

/** The server's copy of an edited file changed meanwhile. Escape keeps the server's version. */
export function EditConflictDialog({ tabId }: { tabId: string }) {
  const file = useEditsStore((s) => conflictOf(s.byTab[tabId]));
  const keepRef = useRef<HTMLButtonElement>(null);
  if (!file) return null;
  const texts = t.sftp.edit;
  const answer = (overwrite: boolean) => void useEditsStore.getState().resolve(tabId, file.id, overwrite);
  return (
    <Dialog
      open
      size="sm"
      title={texts.conflictTitle}
      onClose={() => answer(false)}
      initialFocusRef={keepRef}
      footer={
        <>
          <span className={styles.footerGap} />
          <Button ref={keepRef} onClick={() => answer(false)}>{texts.keep}</Button>
          <Button variant="primary" onClick={() => answer(true)}>{texts.overwrite}</Button>
        </>
      }
    >
      <p>{texts.conflictText(file.name)}</p>
    </Dialog>
  );
}
