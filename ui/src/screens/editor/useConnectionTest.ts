import { useCallback, useEffect, useRef, useState } from "react";
import type { Channel } from "@tauri-apps/api/core";
import {
  type ConnectEvent,
  type CoreError,
  connectAnswer,
  connectCancel,
  connectTest,
  createEventChannel,
  type Host,
  newId,
  type Prompt,
  type PromptAnswer,
} from "../../ipc/api";
import { applyConnectEvent, asCoreError, EMPTY_PROGRESS, fireAndForget, type StepProgress } from "../../stores/connectProgress";
import type { AuthDraft } from "./drafts";

export type TestStatus = "idle" | "running" | "ok" | "failed";

export interface TestState {
  status: TestStatus;
  progress: StepProgress;
  prompt?: { promptId: number; prompt: Prompt };
  error?: CoreError;
  /** What the command resolved with (e.g. `keyCopyId`: added or not). */
  result?: unknown;
}

const IDLE: TestState = { status: "idle", progress: EMPTY_PROGRESS };

/** Secrets typed into the (unsaved) draft answer the first password/passphrase prompt of a test. */
export function autoAnswer(prompt: Prompt, auth: AuthDraft | null, passphraseAnswered = false): PromptAnswer | null {
  if (!auth) return null;
  if (prompt.kind === "password" && !prompt.retry && auth.password) return { kind: "secret", value: auth.password, save: false };
  if (prompt.kind === "passphrase" && !passphraseAnswered && auth.passphrase) return { kind: "secret", value: auth.passphrase, save: false };
  return null;
}

/** "Verbindung testen": runs `connectTest` for an unsaved host; cancels a running test on unmount. */
export function useConnectionTest() {
  const [state, setState] = useState<TestState>(IDLE);
  const running = useRef<string | null>(null);
  // Only the first passphrase prompt of a run is answered from the draft; a wrong one must not burn every attempt.
  const passphraseAnswered = useRef(false);

  useEffect(
    () => () => {
      const connectId = running.current;
      running.current = null;
      if (connectId) fireAndForget(() => connectCancel(connectId));
    },
    [],
  );

  /** `invoke` runs another pipeline command with the same prompts (e.g. `keyCopyId`); default: `connectTest`. */
  const run = useCallback(
    async (
      host: Host,
      auth: AuthDraft | null,
      invoke: (connectId: string, events: Channel<ConnectEvent>) => Promise<unknown> = (id, events) => connectTest(id, host, events),
    ) => {
    const connectId = newId();
    running.current = connectId;
    passphraseAnswered.current = false;
    setState({ status: "running", progress: EMPTY_PROGRESS });
    const events = createEventChannel<ConnectEvent>((event) => {
      if (running.current !== connectId) return;
      if (event.kind === "prompt") {
        if (event.prompt.kind === "hostKeyChanged") {
          // Fail closed: a changed host key aborts the test; the panel only explains why.
          fireAndForget(() => connectAnswer(event.promptId, { kind: "hostKey", decision: "reject" }));
        }
        const answer = autoAnswer(event.prompt, auth, passphraseAnswered.current);
        if (answer) {
          if (event.prompt.kind === "passphrase") passphraseAnswered.current = true;
          fireAndForget(() => connectAnswer(event.promptId, answer));
          return;
        }
        setState((current) => ({ ...current, prompt: { promptId: event.promptId, prompt: event.prompt } }));
        return;
      }
      setState((current) => ({ ...current, progress: applyConnectEvent(current.progress, event) }));
    });
    try {
      const result = await invoke(connectId, events);
      if (running.current === connectId) setState((current) => ({ ...current, status: "ok", prompt: undefined, result }));
    } catch (error) {
      if (running.current === connectId) {
        setState((current) => ({ ...current, status: "failed", error: asCoreError(error), prompt: undefined }));
      }
    } finally {
      if (running.current === connectId) running.current = null;
    }
    },
    [],
  );

  const answer = useCallback((promptId: number, value: PromptAnswer) => {
    setState((current) => ({ ...current, prompt: undefined }));
    fireAndForget(() => connectAnswer(promptId, value));
  }, []);

  const cancel = useCallback(() => {
    const connectId = running.current;
    running.current = null;
    setState(IDLE);
    if (connectId) fireAndForget(() => connectCancel(connectId));
  }, []);

  return { state, run, answer, cancel };
}
