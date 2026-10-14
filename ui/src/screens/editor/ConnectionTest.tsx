import { Zap } from "lucide-react";
import { Button, cx, StepList } from "../../components";
import { t } from "../../i18n/de";
import type { PromptAnswer } from "../../ipc/api";
import { isInlinePrompt, PromptForm } from "../connect/PromptForm";
import { errorTitle, stepItems } from "../connect/steps";
import styles from "./Editor.module.css";
import type { TestState } from "./useConnectionTest";

/** Footer part: "Verbindung testen" + one-line result. */
export function ConnectionTestStatus({
  state,
  disabled,
  onRun,
  onCancel,
}: {
  state: TestState;
  disabled: boolean;
  onRun: () => void;
  onCancel: () => void;
}) {
  const running = state.status === "running";
  const text =
    state.status === "running"
      ? t.editor.testRunning
      : state.status === "ok"
        ? t.editor.testOk
        : state.status === "failed" && state.error
          ? t.editor.testFailed(errorTitle(state.error))
          : "";
  return (
    <>
      {running ? (
        <Button variant="ghost" onClick={onCancel}>
          {t.editor.testCancel}
        </Button>
      ) : (
        <Button icon={Zap} disabled={disabled} onClick={onRun}>
          {t.editor.test}
        </Button>
      )}
      <span
        role="status"
        className={cx(styles.status, state.status === "ok" && styles.statusOk, state.status === "failed" && styles.statusFailed)}
      >
        {text}
      </span>
    </>
  );
}

/** Body part: steps of the running/finished test and inline prompts. */
export function ConnectionTestPanel({
  state,
  jumpName,
  onAnswer,
}: {
  state: TestState;
  jumpName?: string;
  onAnswer: (promptId: number, answer: PromptAnswer) => void;
}) {
  if (state.status === "idle") return null;
  const pending = state.prompt;
  let detail = null;
  if (pending) {
    const { promptId, prompt } = pending;
    if (prompt.kind === "hostKeyUnknown") {
      detail = (
        <div className={styles.hostKey}>
          <span className="mono">{t.editor.hostKeyUnknown(prompt.info.sha256)}</span>
          <Button size="sm" variant="ghost" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "reject" })}>
            {t.common.cancel}
          </Button>
          <Button size="sm" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "acceptOnce" })}>
            {t.editor.acceptOnce}
          </Button>
          <Button size="sm" variant="primary" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "trust" })}>
            {t.editor.trust}
          </Button>
        </div>
      );
    } else if (prompt.kind === "hostKeyChanged") {
      detail = (
        <div className={styles.hostKey}>
          <span>{t.editor.hostKeyChanged}</span>
          <Button size="sm" variant="inverted" onClick={() => onAnswer(promptId, { kind: "hostKey", decision: "reject" })}>
            {t.common.cancel}
          </Button>
        </div>
      );
    } else if (isInlinePrompt(prompt)) {
      detail = <PromptForm prompt={prompt} showSave={false} onAnswer={(answer) => onAnswer(promptId, answer)} />;
    }
  }
  return (
    <section className={styles.test} aria-label={t.editor.testSteps}>
      <StepList steps={stepItems(state.progress, { jumpName, detail })} />
      {detail && !state.progress.current ? detail : null}
    </section>
  );
}
