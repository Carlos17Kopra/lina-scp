import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { AuthMethodFields } from "./AuthMethodFields";
import type { AuthDraft } from "./drafts";

vi.mock("../../ipc/api");
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const draft: AuthDraft = { method: "key", keyPath: "", password: "", passphrase: "", hasSecret: false };

beforeEach(() => {
  vi.mocked(api.keyDefaults).mockResolvedValue({ name: "id_ed25519", comment: "me@box" });
  vi.mocked(api.keyGenerate).mockResolvedValue({
    path: "/home/me/.ssh/id_ed25519",
    name: "id_ed25519",
    algorithm: "ssh-ed25519",
    fingerprint: "SHA256:x",
    comment: "me@box",
    publicKey: "ssh-ed25519 AAAA me@box",
    encrypted: true,
  });
});

describe("AuthMethodFields", () => {
  it("generates a key and takes over its path and passphrase", async () => {
    const onChange = vi.fn();
    render(
      <ToastProvider>
        <AuthMethodFields auth={draft} onChange={onChange} />
      </ToastProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Neu erzeugen" }));
    const dialog = await screen.findByRole("dialog", { name: "Neuer Schlüssel" });
    await within(dialog).findByDisplayValue("id_ed25519");
    await userEvent.type(within(dialog).getByLabelText("Passphrase"), "geheim");
    await userEvent.type(within(dialog).getByLabelText("Passphrase wiederholen"), "geheim");
    await userEvent.click(within(dialog).getByRole("button", { name: "Erzeugen" }));
    await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith({ ...draft, keyPath: "/home/me/.ssh/id_ed25519", passphrase: "geheim" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
