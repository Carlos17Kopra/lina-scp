import { type InputHTMLAttributes, type Ref, useId } from "react";
import { cx } from "../cx";
import { fieldDescription } from "../field";
import field from "../field.module.css";

export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "size"> {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string | null;
  hint?: string;
  mono?: boolean;
  ref?: Ref<HTMLInputElement>;
}

export function TextField({
  label,
  value,
  onChange,
  error,
  hint,
  mono = false,
  id,
  className,
  type = "text",
  ...rest
}: TextFieldProps) {
  const generatedId = useId();
  const inputId = id ?? generatedId;
  const { hintId, errorId, describedBy } = fieldDescription(inputId, hint, error);
  return (
    <div className={cx(field.field, className)}>
      <label className={field.label} htmlFor={inputId}>
        {label}
      </label>
      <input
        {...rest}
        id={inputId}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy}
        className={cx(field.control, mono && field.mono, error && field.invalid)}
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
