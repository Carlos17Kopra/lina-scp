import { type KeyboardEvent, useRef, useState } from "react";
import { Button, Dialog, TextField } from "../../components";
import { t } from "../../i18n/de";
import styles from "./Snippets.module.css";

export type SnippetMode = "insert" | "run";

/** One field per placeholder name; values are never stored. */
export function PlaceholderDialog({
  names,
  mode,
  onConfirm,
  onCancel,
}: {
  names: string[];
  mode: SnippetMode;
  onConfirm: (values: Record<string, string>) => void;
  onCancel: () => void;
}) {
  const texts = t.snippets.placeholders;
  const firstRef = useRef<HTMLInputElement>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  // Enter in any field confirms; the submit button lives in the dialog footer, outside the fields.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    onConfirm(values);
  };
  return (
    <Dialog
      open
      size="sm"
      title={texts.title}
      onClose={onCancel}
      initialFocusRef={firstRef}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" onClick={() => onConfirm(values)}>
            {mode === "run" ? texts.run : texts.insert}
          </Button>
        </>
      }
    >
      <div className={styles.form} onKeyDown={onKeyDown}>
        {names.map((name, index) => (
          <TextField
            key={name}
            ref={index === 0 ? firstRef : undefined}
            label={name}
            mono
            value={values[name] ?? ""}
            onChange={(value) => setValues((previous) => ({ ...previous, [name]: value }))}
          />
        ))}
      </div>
    </Dialog>
  );
}
