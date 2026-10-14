import { type KeyboardEvent, useRef } from "react";
import { cx, nextIndex } from "../../components";
import { t } from "../../i18n/de";
import { HOST_COLORS, type HostColor } from "../../model/hostFormat";
import styles from "./Editor.module.css";

export function ColorSwatches({ value, onChange }: { value: HostColor; onChange: (color: HostColor) => void }) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, HOST_COLORS.length, "horizontal");
    const color = next === null ? undefined : HOST_COLORS[next];
    if (next === null || !color) return;
    event.preventDefault();
    onChange(color);
    refs.current[next]?.focus();
  };

  return (
    <div role="radiogroup" aria-label={t.editor.color} className={styles.swatches}>
      {HOST_COLORS.map((color, index) => {
        const checked = color === value;
        return (
          <button
            key={color}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={t.editor.colors[color]}
            title={t.editor.colors[color]}
            tabIndex={checked ? 0 : -1}
            className={cx(styles.swatch, checked && styles.swatchChecked)}
            style={{ background: `var(--${color})` }}
            onClick={() => onChange(color)}
            onKeyDown={(event) => onKeyDown(event, index)}
          />
        );
      })}
    </div>
  );
}
