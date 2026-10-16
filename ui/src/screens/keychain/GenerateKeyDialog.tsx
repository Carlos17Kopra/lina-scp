import { useEffect, useState } from "react";
import { Button, Dialog, Segmented, TextField } from "../../components";
import { t } from "../../i18n/de";
import { type KeyInfo, type KeyKind, keyDefaults, keyGenerate } from "../../ipc/api";
import { errorMessage } from "../../ipc/errors";
import styles from "./Keychain.module.css";

const KINDS: ReadonlyArray<{ value: KeyKind; label: string }> = [
  { value: "ed25519", label: "Ed25519" },
  { value: "rsa4096", label: "RSA 4096" },
];

/** "Neuer Schlüssel": writes ~/.ssh/<name> and .pub. `onGenerated` gets the key and the passphrase it was made with. */
export function GenerateKeyDialog({
  onClose,
  onGenerated,
}: {
  onClose: () => void;
  onGenerated: (key: KeyInfo, passphrase: string) => void;
}) {
  const texts = t.keychain.generate;
  const [kind, setKind] = useState<KeyKind>("ed25519");
  const [name, setName] = useState("");
  // A typed name wins over every later suggestion.
  const [nameTyped, setNameTyped] = useState(false);
  const [comment, setComment] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    keyDefaults(kind)
      .then((defaults) => {
        if (!current) return;
        if (!nameTyped) setName(defaults.name);
        setComment((previous) => previous || defaults.comment);
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [kind, nameTyped]);

  const mismatch = repeat !== passphrase;
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const key = await keyGenerate(kind, name.trim(), comment.trim(), passphrase === "" ? null : passphrase);
      onGenerated(key, passphrase);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      size="sm"
      title={texts.title}
      // A running generation cannot be called back: the files are written anyway.
      onClose={busy ? () => {} : onClose}
      footer={
        <>
          <Button variant="ghost" disabled={busy} onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" disabled={busy || mismatch || name.trim() === ""} onClick={() => void submit()}>
            {texts.submit}
          </Button>
        </>
      }
    >
      <div className={styles.form}>
        <Segmented label={texts.kind} value={kind} options={KINDS} onChange={setKind} />
        <TextField label={texts.name} mono spellCheck={false} value={name}
          onChange={(value) => {
            setNameTyped(true);
            setName(value);
          }} hint={texts.where(name.trim())} />
        <TextField label={texts.comment} value={comment} onChange={setComment} />
        <TextField type="password" label={texts.passphrase} autoComplete="new-password" hint={texts.hint} value={passphrase} onChange={setPassphrase} />
        <TextField
          type="password"
          label={texts.repeat}
          autoComplete="new-password"
          error={mismatch && repeat !== "" ? texts.mismatch : undefined}
          value={repeat}
          onChange={setRepeat}
        />
        {error ? (
          <p role="alert" className={styles.error}>
            {texts.failed(error)}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}
