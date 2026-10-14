import { ChevronDown } from "lucide-react";
import { type SelectHTMLAttributes, useId } from "react";
import { cx } from "../cx";
import { fieldDescription } from "../field";
import field from "../field.module.css";
import styles from "./Select.module.css";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SelectProps<T extends string>
  extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "value" | "onChange"> {
  label: string;
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  hint?: string;
  error?: string | null;
}

export function Select<T extends string>({
  label,
  value,
  options,
  onChange,
  hint,
  error,
  id,
  className,
  ...rest
}: SelectProps<T>) {
  const generatedId = useId();
  const selectId = id ?? generatedId;
  const { hintId, errorId, describedBy } = fieldDescription(selectId, hint, error);
  return (
    <div className={cx(field.field, className)}>
      <label className={field.label} htmlFor={selectId}>
        {label}
      </label>
      <div className={styles.wrap}>
        <select
          {...rest}
          id={selectId}
          value={value}
          onChange={(event) => onChange(event.target.value as T)}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cx(field.control, styles.select, error && field.invalid)}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value} disabled={option.disabled}>
              {option.label}
            </option>
          ))}
        </select>
        <ChevronDown className={styles.chevron} size={16} aria-hidden="true" />
      </div>
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
