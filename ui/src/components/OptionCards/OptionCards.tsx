import { type KeyboardEvent, useId, useRef } from "react";
import { cx } from "../cx";
import { nextIndex } from "../roving";
import styles from "./OptionCards.module.css";

export interface OptionCardItem<T extends string> {
  value: T;
  label: string;
  hint?: string;
}

export interface OptionCardsProps<T extends string> {
  /** Accessible name of the radiogroup. */
  label: string;
  value: T;
  options: readonly OptionCardItem<T>[];
  onChange: (value: T) => void;
  className?: string;
}

/** Radiogroup of bordered cards (label + hint), e.g. "Login-Profil verwenden" / "Individuell". */
export function OptionCards<T extends string>({ label, value, options, onChange, className }: OptionCardsProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const baseId = useId();
  const selectedIndex = options.findIndex((option) => option.value === value);
  const tabStop = selectedIndex >= 0 ? selectedIndex : 0;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, options.length, "both");
    const option = next === null ? undefined : options[next];
    if (next === null || !option) return;
    event.preventDefault();
    onChange(option.value);
    refs.current[next]?.focus();
  };

  return (
    <div role="radiogroup" aria-label={label} className={cx(styles.group, className)}>
      {options.map((option, index) => {
        const checked = option.value === value;
        const labelId = `${baseId}-${index}-label`;
        const hintId = `${baseId}-${index}-hint`;
        return (
          <button
            key={option.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-labelledby={labelId}
            aria-describedby={option.hint ? hintId : undefined}
            tabIndex={index === tabStop ? 0 : -1}
            className={cx(styles.card, checked && styles.checked)}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            <span id={labelId} className={styles.label}>
              {option.label}
            </span>
            {option.hint ? (
              <span id={hintId} className={styles.hint}>
                {option.hint}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
