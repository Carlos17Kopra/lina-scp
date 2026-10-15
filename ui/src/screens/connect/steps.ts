import type { ReactNode } from "react";
import type { StepItem, StepState } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Step } from "../../ipc/api";
import type { StepProgress } from "../../stores/connectProgress";

export function stepLabel(step: Step, jumpName = ""): string {
  switch (step) {
    case "jumpHost":
      return t.pipeline.steps.jumpHost(jumpName);
    case "tcp":
      return t.pipeline.steps.tcp;
    case "hostKey":
      return t.pipeline.steps.hostKey;
    case "auth":
      return t.pipeline.steps.auth;
    case "channel":
      return t.pipeline.steps.channel;
    case "shell":
      return t.pipeline.steps.shell;
    case "subsystem":
      return t.pipeline.steps.subsystem;
    case "directory":
      return t.pipeline.steps.directory;
  }
}

export function stepState(progress: StepProgress, step: Step): StepState {
  if (progress.failedStep === step) return "failed";
  if (progress.done.includes(step)) return "done";
  if (progress.current === step) return "active";
  return "pending";
}

export interface StepItemOptions {
  jumpName?: string;
  /** Mono text shown once a step is reached. */
  meta?: Partial<Record<Step, string>>;
  /** Rendered below the active step (inline prompts). */
  detail?: ReactNode;
}

export function stepItems(progress: StepProgress, options: StepItemOptions = {}): StepItem[] {
  return progress.steps.map((step) => {
    const state = stepState(progress, step);
    return {
      id: step,
      label: stepLabel(step, options.jumpName),
      state,
      meta: state === "pending" ? undefined : options.meta?.[step],
      detail: state === "active" ? options.detail : undefined,
    };
  });
}

/** Short German title of a pipeline error (backend messages of other categories are shown as-is). */
export function errorTitle(error: CoreError): string {
  switch (error.type) {
    case "network":
      return error.kind === "other" ? t.pipeline.errors.network : t.pipeline.errors[error.kind];
    case "auth":
      return t.pipeline.errors.auth;
    case "hostKeyRejected":
      return t.pipeline.errors.hostKeyRejected;
    case "cancelled":
      return t.pipeline.errors.cancelled;
    case "inputRequired":
      return t.pipeline.errors.inputRequired;
    default:
      return error.message;
  }
}
