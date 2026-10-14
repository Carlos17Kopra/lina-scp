import { create } from "zustand";
import type { CoreError, Id, Prompt, PromptAnswer, Step } from "../ipc/api";

export type ConnectProtocol = "ssh" | "sftp";

export interface ConnectFlow {
  connectId: string;
  hostId: Id;
  protocol: ConnectProtocol;
  steps: Step[];
  current?: Step;
  done: Step[];
  failed?: { step: Step; error: CoreError };
  log: string[];
  prompt?: { promptId: number; prompt: Prompt };
  attempt: number;
  autoRetry: boolean;
}

export interface ConnectState {
  active: ConnectFlow | null;
  start(hostId: Id, protocol: ConnectProtocol): Promise<void>;
  answer(promptId: number, answer: PromptAnswer): Promise<void>;
  cancel(): Promise<void>;
  retry(): Promise<void>;
}

/** State shape only — Task 21 implements the connection flow. */
export const useConnectStore = create<ConnectState>()(() => ({
  active: null,
  start: async () => undefined,
  answer: async () => undefined,
  cancel: async () => undefined,
  retry: async () => undefined,
}));
