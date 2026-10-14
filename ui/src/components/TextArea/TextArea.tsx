import { type Ref, type TextareaHTMLAttributes, useId } from "react";
import { cx } from "../cx";
import { fieldDescription } from "../field";
import field from "../field.module.css";
import styles from "./TextArea.module.css";

export interface TextAreaProps extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "onChange"> {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string | null;
  hint?: string;
  mono?: boolean;
  ref?: Ref<HTMLTextAreaElement>;
}

export function TextArea({ label, value, onChange, error, hint, mono = false, id, className, rows = 3, ...rest }: TextAreaProps) {
  const generatedId = useId();
  const areaId = id ?? generatedId;
  const { hintId, errorId, describedBy } = fieldDescription(areaId, hint, error);
  return (
    <div className={cx(field.field, className)}>
      <label className={field.label} htmlFor={areaId}>
        {label}
      </label>
      <textarea
        {...rest}
        id={areaId}
        rows={rows}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy}
        className={cx(field.control, styles.area, mono && field.mono, error && field.invalid)}
      />
      {hint ? (
        <p id={hintId} className={field.hint}>
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className={field.error}>
          {error}
        </p>
      ) : null}
    </div>
  );
}
