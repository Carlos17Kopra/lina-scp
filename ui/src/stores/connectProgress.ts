import type { ConnectEvent, CoreError, Step } from "../ipc/api";
import { errorMessage, isCoreError } from "../ipc/errors";

/** Progress of one pipeline run, derived from `ConnectEvent`s (shared by the connect flow and "Verbindung testen"). */
export interface StepProgress {
  steps: Step[];
  current?: Step;
  done: Step[];
  failedStep?: Step;
  log: string[];
}

export const EMPTY_PROGRESS: StepProgress = { steps: [], done: [], log: [] };
export const MAX_LOG_LINES = 500;

/** Reduces progress events; `prompt`, `connected` and `failed` are handled by the caller and return `state` unchanged. */
export function applyConnectEvent<T extends StepProgress>(state: T, event: ConnectEvent): T {
  switch (event.kind) {
    case "steps":
      return { ...state, steps: event.steps, done: [], current: undefined, failedStep: undefined };
    case "stepStarted":
      return { ...state, current: event.step };
    case "stepDone":
      return {
        ...state,
        done: state.done.includes(event.step) ? state.done : [...state.done, event.step],
        current: state.current === event.step ? undefined : state.current,
      };
    case "stepFailed":
      return { ...state, current: undefined, failedStep: event.step };
    case "log": {
      const log = [...state.log, event.line];
      return { ...state, log: log.length > MAX_LOG_LINES ? log.slice(log.length - MAX_LOG_LINES) : log };
    }
    default:
      return state;
  }
}

export function progressRatio(progress: StepProgress): number {
  return progress.steps.length === 0 ? 0 : progress.done.length / progress.steps.length;
}

/** Command rejections are `CoreError`s; anything else (bugs, mocks) becomes `internal`. */
export function asCoreError(error: unknown): CoreError {
  return isCoreError(error) ? error : { type: "internal", message: errorMessage(error) };
}

/** Runs a best-effort IPC call whose outcome does not matter (cancel, answer of a stale prompt). */
export function fireAndForget(task: () => Promise<unknown> | undefined): void {
  void (async () => {
    try {
      await task();
    } catch {
      // ignored on purpose
    }
  })();
}
