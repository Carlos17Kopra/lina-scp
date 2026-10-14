import { cx } from "../cx";
import type { Tone } from "../tone";
import styles from "./ProgressBar.module.css";

export interface ProgressBarProps {
  /** 0..1 */
  value: number;
  tone?: Tone;
  label?: string;
  className?: string;
}

export function ProgressBar({ value, tone = "accent", label, className }: ProgressBarProps) {
  const ratio = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  const percent = Math.round(ratio * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      className={cx(styles.track, className)}
    >
      <div className={cx(styles.fill, styles[tone])} style={{ width: `${percent}%` }} />
    </div>
  );
}
