import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { Button, Segmented, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { AuthMethod } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import type { AuthDraft } from "./drafts";
import styles from "./Editor.module.css";

const METHOD_OPTIONS: ReadonlyArray<{ value: AuthMethod; label: string }> = [
  { value: "password", label: t.auth.password },
  { value: "key", label: t.auth.key },
  { value: "agent", label: t.auth.agent },
  { value: "keyboardInteractive", label: t.auth.keyboardInteractive },
];

/** Method switch + method specific fields (shared by individual host auth and login profiles). */
export function AuthMethodFields({
  auth,
  onChange,
  keyPathError,
}: {
  auth: AuthDraft;
  onChange: (auth: AuthDraft) => void;
  keyPathError?: string;
}) {
  const toast = useToast();
  const set = (patch: Partial<AuthDraft>) => onChange({ ...auth, ...patch });

  const pickKey = async () => {
    try {
      const path = await open({ multiple: false, directory: false, title: t.editor.pickKeyTitle });
      if (typeof path === "string") set({ keyPath: path });
    } catch (error) {
      toast.show({ tone: "red", message: t.editor.pickKeyFailed, details: errorDetails(error) });
    }
  };

  return (
    <>
      <div className={styles.fieldset}>
        <span className={styles.legend} aria-hidden="true">
          {t.editor.method}
        </span>
        <Segmented label={t.editor.method} value={auth.method} options={METHOD_OPTIONS} onChange={(method) => set({ method })} />
      </div>
      {auth.method === "password" ? (
        <TextField
          type="password"
          label={t.editor.password}
          autoComplete="new-password"
          placeholder={auth.hasSecret ? t.editor.secretStored : undefined}
          hint={t.editor.secretHint}
          value={auth.password}
          onChange={(password) => set({ password })}
        />
      ) : null}
      {auth.method === "key" ? (
        <>
          <div className={styles.inline}>
            <TextField
              label={t.editor.keyPath}
              mono
              spellCheck={false}
              placeholder={t.editor.keyPathPlaceholder}
              value={auth.keyPath}
              error={keyPathError}
              onChange={(keyPath) => set({ keyPath })}
            />
            <Button icon={FolderOpen} onClick={() => void pickKey()}>
              {t.editor.pickKey}
            </Button>
          </div>
          <TextField
            type="password"
            label={t.editor.passphrase}
            autoComplete="new-password"
            placeholder={auth.hasSecret ? t.editor.secretStored : t.editor.passphrasePlaceholder}
            hint={t.editor.secretHint}
            value={auth.passphrase}
            onChange={(passphrase) => set({ passphrase })}
          />
        </>
      ) : null}
      {auth.method === "agent" ? <p className={styles.info}>{t.editor.agentInfo}</p> : null}
      {auth.method === "keyboardInteractive" ? <p className={styles.info}>{t.editor.kbdInfo}</p> : null}
    </>
  );
}
