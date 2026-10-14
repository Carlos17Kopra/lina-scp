import { t } from "../../i18n/de";
import type { CoreError, Host } from "../../ipc/api";
import type { ConnectFlow } from "../../stores/connect";

export type ConnectView = "progress" | "hostKeyNew" | "hostKeyChanged" | "errorConnection" | "errorAuth";

/** Which screen the connect overlay shows: host-key prompts first, inline prompts keep the progress, then errors. */
export function connectView(flow: ConnectFlow): ConnectView {
  const prompt = flow.prompt?.prompt;
  if (prompt?.kind === "hostKeyUnknown") return "hostKeyNew";
  if (prompt?.kind === "hostKeyChanged") return "hostKeyChanged";
  if (prompt) return "progress";
  if (flow.failed) return flow.failed.error.type === "auth" ? "errorAuth" : "errorConnection";
  return "progress";
}

/** `ssh-ed25519` → `ED25519` (OpenSSH style). */
export function algorithmLabel(algorithm: string): string {
  return algorithm.replace(/^ssh-/, "").toUpperCase();
}

/** The classic OpenSSH banner shown under "Technische Details". */
export function openSshWarning(algorithm: string): string {
  return [
    "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@",
    "@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @",
    "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@",
    `Offending ${algorithmLabel(algorithm)} key in ~/.ssh/known_hosts`,
    "Host key verification failed.",
  ].join("\n");
}

export interface ConnectionErrorText {
  title: string;
  message: string;
  causes: string[];
  /** Original backend message (client-style one-liner). */
  detail?: string;
}

export function connectionErrorText(error: CoreError, host: Host): ConnectionErrorText {
  const texts = t.connect.error;
  const detail = "message" in error && error.message ? error.message : undefined;
  if (error.type === "network") {
    switch (error.kind) {
      case "timeout":
        return { title: texts.timeout.title, message: texts.timeout.message(host.timeoutS), causes: [...texts.timeout.causes(host.port)], detail };
      case "refused":
        return { title: texts.refused.title, message: texts.refused.message(host.port), causes: [...texts.refused.causes], detail };
      case "dns":
        return { title: texts.dns.title, message: texts.dns.message, causes: [...texts.dns.causes], detail };
      case "other":
        break;
    }
  }
  return { title: texts.other.title, message: texts.other.message, causes: [], detail };
}
