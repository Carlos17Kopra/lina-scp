import { Check, LoaderCircle, X } from "lucide-react";
import type { ReactNode } from "react";
import { t } from "../../i18n/de";
import { cx } from "../cx";
import styles from "./StepList.module.css";

export type StepState = "pending" | "active" | "done" | "failed";

export interface StepItem {
  id: string;
  label: string;
  state: StepState;
  /** Mono text on the right, e.g. "admin@bastion:2222". */
  meta?: string;
  /** Rendered below the step, e.g. an inline keyboard-interactive prompt. */
  detail?: ReactNode;
}

export interface StepListProps {
  steps: readonly StepItem[];
  className?: string;
}

function StepIcon({ state }: { state: StepState }) {
  switch (state) {
    case "done":
      return <Check size={16} strokeWidth={2.6} />;
    case "active":
      return <LoaderCircle size={16} strokeWidth={2.4} className={styles.spin} />;
    case "failed":
      return <X size={16} strokeWidth={2.6} />;
    case "pending":
      return <span className={styles.dot} />;
  }
}

export function StepList({ steps, className }: StepListProps) {
  return (
    <ol className={cx(styles.list, className)}>
      {steps.map((step) => (
        <li
          key={step.id}
          data-state={step.state}
          aria-current={step.state === "active" ? "step" : undefined}
          className={cx(styles.step, styles[step.state])}
        >
          <span className={styles.icon} aria-hidden="true">
            <StepIcon state={step.state} />
          </span>
          <span className={styles.label}>
            {step.label}
            <span className="visually-hidden">{` – ${t.steps[step.state]}`}</span>
          </span>
          {step.meta ? <span className={styles.meta}>{step.meta}</span> : null}
          {step.detail ? <div className={styles.detail}>{step.detail}</div> : null}
        </li>
      ))}
    </ol>
  );
}
