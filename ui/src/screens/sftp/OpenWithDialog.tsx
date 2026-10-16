import { useEffect, useRef, useState } from "react";
import { Button, Checkbox, Dialog } from "../../components";
import { t } from "../../i18n/de";
import { type OpenApp, type OpenWithApps, openWithApps } from "../../ipc/api";
import styles from "./SftpScreen.module.css";

/** Picks an app for one file; the switch makes it the default for the file type (in LinaSCP only). */
export function OpenWithDialog({ name, onOpen, onClose }: {
  name: string;
  onOpen: (how: OpenApp) => void;
  onClose: () => void;
}) {
  const texts = t.sftp.openWith;
  const openRef = useRef<HTMLButtonElement>(null);
  const [choices, setChoices] = useState<OpenWithApps | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [remember, setRemember] = useState(false);

  useEffect(() => {
    let live = true;
    openWithApps(name)
      .then((result) => {
        if (!live) return;
        const first = result.apps.find((a) => a.id === result.remembered) ?? result.apps[0];
        setChoices(result);
        setSelected(first?.id ?? null);
        setRemember(first !== undefined && first.id === result.remembered);
      })
      .catch(() => live && setChoices({ typeName: "", apps: [], remembered: null }));
    return () => {
      live = false;
    };
  }, [name]);

  const pick = (id: string) => {
    setSelected(id);
    setRemember(id === choices?.remembered);
  };
  const open = () => {
    if (!selected) return;
    onOpen({ kind: "app", id: selected, remember });
    onClose();
  };

  return (
    <Dialog
      open
      size="sm"
      title={texts.title(name)}
      onClose={onClose}
      initialFocusRef={openRef}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button ref={openRef} variant="primary" disabled={!selected} onClick={open}>{texts.open}</Button>
        </>
      }
    >
      {choices === null ? <p>{t.sftp.loading}</p> : null}
      {choices && choices.apps.length === 0 ? <p>{texts.empty}</p> : null}
      {choices && choices.apps.length > 0 ? (
        <>
          <div role="radiogroup" aria-label={texts.apps} className={styles.appList}>
            {choices.apps.map((app) => (
              <label key={app.id} className={styles.appChoice} onDoubleClick={open}>
                <input type="radio" name="open-with" checked={selected === app.id} onChange={() => pick(app.id)} />
                {app.name}
              </label>
            ))}
          </div>
          <Checkbox label={texts.remember(choices.typeName)} hint={texts.rememberHint} checked={remember} onChange={setRemember} />
        </>
      ) : null}
    </Dialog>
  );
}
