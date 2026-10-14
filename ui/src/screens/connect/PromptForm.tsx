import { type FormEvent, useState } from "react";
import { Button, Checkbox, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { Prompt, PromptAnswer } from "../../ipc/api";
import styles from "./PromptForm.module.css";

export type InlinePrompt = Extract<Prompt, { kind: "password" | "passphrase" | "keyboardInteractive" }>;

export function isInlinePrompt(prompt: Prompt): prompt is InlinePrompt {
  return prompt.kind === "password" || prompt.kind === "passphrase" || prompt.kind === "keyboardInteractive";
}

export interface PromptFormProps {
  prompt: InlinePrompt;
  onAnswer: (answer: PromptAnswer) => void;
  /** Offer "im Schlüsselbund speichern" (password/passphrase). */
  showSave?: boolean;
}

/** Inline answer form for password, passphrase and keyboard-interactive (2FA) prompts. */
export function PromptForm({ prompt, onAnswer, showSave = true }: PromptFormProps) {
  return prompt.kind === "keyboardInteractive" ? (
    <KeyboardInteractiveForm prompt={prompt} onAnswer={onAnswer} />
  ) : (
    <SecretForm prompt={prompt} onAnswer={onAnswer} showSave={showSave} />
  );
}

function SecretForm({
  prompt,
  onAnswer,
  showSave,
}: {
  prompt: Extract<Prompt, { kind: "password" | "passphrase" }>;
  onAnswer: (answer: PromptAnswer) => void;
  showSave: boolean;
}) {
  const [value, setValue] = useState("");
  const [save, setSave] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const isPassword = prompt.kind === "password";
  const label = isPassword ? t.pipeline.prompt.password(`${prompt.user}@${prompt.host}`) : t.pipeline.prompt.passphrase(prompt.keyPath);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!value) {
      setError(t.pipeline.prompt.empty);
      return;
    }
    onAnswer({ kind: "secret", value, save: showSave && save });
  };

  return (
    <form className={styles.form} onSubmit={submit}>
      {isPassword && prompt.retry ? <p className={styles.retry}>{t.pipeline.prompt.passwordRetry}</p> : null}
      <TextField
        type="password"
        label={label}
        value={value}
        autoFocus
        autoComplete="off"
        error={error}
        onChange={(next) => {
          setValue(next);
          setError(null);
        }}
      />
      {showSave ? (
        <Checkbox
          label={isPassword ? t.pipeline.prompt.savePassword : t.pipeline.prompt.savePassphrase}
          checked={save}
          onChange={setSave}
        />
      ) : null}
      <div className={styles.actions}>
        <Button size="sm" variant="ghost" onClick={() => onAnswer({ kind: "cancel" })}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit">
          {t.pipeline.prompt.submit}
        </Button>
      </div>
    </form>
  );
}

function KeyboardInteractiveForm({
  prompt,
  onAnswer,
}: {
  prompt: Extract<Prompt, { kind: "keyboardInteractive" }>;
  onAnswer: (answer: PromptAnswer) => void;
}) {
  const [values, setValues] = useState<string[]>(() => prompt.prompts.map(() => ""));

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onAnswer({ kind: "responses", values });
  };

  return (
    <form className={styles.form} onSubmit={submit}>
      {prompt.name ? <p className={styles.name}>{prompt.name}</p> : null}
      {prompt.instructions ? <p className={styles.instructions}>{prompt.instructions}</p> : null}
      {prompt.prompts.map((item, index) => (
        <TextField
          // Prompts have no identity besides their position.
          key={index}
          type={item.echo ? "text" : "password"}
          label={item.text.trim().replace(/:$/, "") || t.pipeline.prompt.response(index + 1)}
          value={values[index] ?? ""}
          autoFocus={index === 0}
          autoComplete="off"
          onChange={(next) => setValues((current) => current.map((v, i) => (i === index ? next : v)))}
        />
      ))}
      <div className={styles.actions}>
        <Button size="sm" variant="ghost" onClick={() => onAnswer({ kind: "cancel" })}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit">
          {t.pipeline.prompt.send}
        </Button>
      </div>
    </form>
  );
}
