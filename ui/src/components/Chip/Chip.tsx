import type { LucideIcon } from "lucide-react";
import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "../cx";
import type { Tone } from "../tone";
import styles from "./Chip.module.css";

export interface ChipProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: Tone;
  icon?: LucideIcon;
  mono?: boolean;
  children: ReactNode;
}

export function Chip({ tone = "neutral", icon: Icon, mono = false, className, children, ...rest }: ChipProps) {
  return (
    <span {...rest} data-tone={tone} className={cx(styles.chip, styles[tone], mono && styles.mono, className)}>
      {Icon ? <Icon size={12} strokeWidth={2.2} aria-hidden="true" /> : null}
      {children}
    </span>
  );
}
