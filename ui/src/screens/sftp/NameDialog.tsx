import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { Button, Dialog, TextField } from "../../components";
import { t } from "../../i18n/de";
import { isCoreError } from "../../ipc/errors";

/** Same rules as core `validate_file_name` (minus U+FFFD, which a user cannot type by accident). */
export function validateName(name: string): string | null {
  const texts = t.sftp.nameErrors;
  if (name.trim() === "") return texts.empty;
  if (name.includes("/")) return texts.slash;
  if (name === "." || name === "..") return texts.dots;
  return null;
}

export function NameDialog({ mode, initial = "", onSubmit, onClose }: {
  mode: "new" | "rename";
  initial?: string;
  onSubmit: (name: string) => Promise<void>;
  onClose: () => void;
}) {
  const texts = t.sftp.nameDialog;
  const ref = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const dot = initial.lastIndexOf(".");
    ref.current?.setSelectionRange(0, dot > 0 ? dot : initial.length);
  }, [initial]);

  const submit = async () => {
    const invalid = validateName(value);
    if (invalid) {
      setError(invalid);
      return;
    }
    if (mode === "rename" && value === initial) {
      onClose();
      return;
    }
    setBusy(true);
    try {
      await onSubmit(value);
      onClose();
    } catch (e) {
      if (isCoreError(e) && e.type === "fs" && e.kind === "alreadyExists") setError(t.sftp.nameErrors.exists);
      else setError(isCoreError(e) && "message" in e ? e.message : t.sftp.failed);
    } finally {
      setBusy(false);
    }
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    event.preventDefault();
    void submit();
  };

  return (
    <Dialog
      open
      size="sm"
      title={mode === "new" ? texts.newTitle : texts.renameTitle}
      onClose={onClose}
      initialFocusRef={ref}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="primary" disabled={busy} onClick={() => void submit()}>
            {mode === "new" ? texts.create : texts.save}
          </Button>
        </>
      }
    >
      <div onKeyDown={onKeyDown}>
        <TextField ref={ref} label={texts.label} mono spellCheck={false} value={value} error={error ?? undefined}
          onChange={(v) => { setValue(v); setError(null); }} />
      </div>
    </Dialog>
  );
}
