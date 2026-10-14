import { secretSet, type SecretOwner } from "../../ipc/api";
import type { AuthDraft } from "./drafts";

/** Writes entered secrets to the keyring after the owner was saved. Returns `false` if the keyring refused. */
export async function storeSecrets(owner: SecretOwner, auth: AuthDraft): Promise<boolean> {
  try {
    if (auth.method === "password" && auth.password) await secretSet(owner, "password", auth.password);
    if (auth.method === "key" && auth.passphrase) await secretSet(owner, "passphrase", auth.passphrase);
    return true;
  } catch {
    return false;
  }
}
