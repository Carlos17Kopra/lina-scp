import type { HTMLAttributes, Ref } from "react";
import { cx } from "../cx";
import styles from "./Card.module.css";

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  selected?: boolean;
  padded?: boolean;
  ref?: Ref<HTMLDivElement>;
}

export function Card({ selected = false, padded = false, className, ...rest }: CardProps) {
  return (
    <div
      {...rest}
      data-selected={selected ? "true" : undefined}
      className={cx(styles.card, selected && styles.selected, padded && styles.padded, className)}
    />
  );
}
