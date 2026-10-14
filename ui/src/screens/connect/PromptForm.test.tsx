import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PromptForm } from "./PromptForm";

describe("PromptForm", () => {
  it("submits a password and the save flag", async () => {
    const onAnswer = vi.fn();
    render(<PromptForm prompt={{ kind: "password", user: "deploy", host: "10.0.1.12", retry: false }} onAnswer={onAnswer} />);
    expect(screen.getByLabelText("Passwort für deploy@10.0.1.12")).toHaveAttribute("type", "password");
    expect(screen.getByRole("checkbox", { name: "Passwort im Schlüsselbund speichern" })).toBeChecked();
    await userEvent.type(screen.getByLabelText("Passwort für deploy@10.0.1.12"), "pw");
    await userEvent.click(screen.getByRole("button", { name: "Anmelden" }));
    expect(onAnswer).toHaveBeenCalledWith({ kind: "secret", value: "pw", save: true });
  });

  it("validates empty input and can be cancelled", async () => {
    const onAnswer = vi.fn();
    render(<PromptForm prompt={{ kind: "passphrase", keyPath: "~/.ssh/id_ed25519" }} onAnswer={onAnswer} />);
    expect(screen.getByRole("checkbox", { name: "Passphrase im Schlüsselbund speichern" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Anmelden" }));
    expect(screen.getByText("Bitte einen Wert eingeben.")).toBeInTheDocument();
    expect(onAnswer).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(onAnswer).toHaveBeenCalledWith({ kind: "cancel" });
  });

  it("shows the retry hint and can hide the save option", async () => {
    const onAnswer = vi.fn();
    render(
      <PromptForm prompt={{ kind: "password", user: "deploy", host: "h", retry: true }} onAnswer={onAnswer} showSave={false} />,
    );
    expect(screen.getByText("Passwort wurde abgelehnt. Bitte erneut eingeben.")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Passwort für deploy@h"), "x{Enter}");
    expect(onAnswer).toHaveBeenCalledWith({ kind: "secret", value: "x", save: false });
  });

  it("answers keyboard-interactive prompts respecting echo", async () => {
    const onAnswer = vi.fn();
    render(
      <PromptForm
        prompt={{
          kind: "keyboardInteractive",
          name: "2FA",
          instructions: "Bitte Code eingeben",
          prompts: [
            { text: "Password: ", echo: false },
            { text: "Verification code: ", echo: true },
          ],
        }}
        onAnswer={onAnswer}
      />,
    );
    expect(screen.getByText("2FA")).toBeInTheDocument();
    expect(screen.getByText("Bitte Code eingeben")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("Verification code")).toHaveAttribute("type", "text");
    await userEvent.type(screen.getByLabelText("Password"), "pw");
    await userEvent.type(screen.getByLabelText("Verification code"), "123456");
    await userEvent.click(screen.getByRole("button", { name: "Senden" }));
    expect(onAnswer).toHaveBeenCalledWith({ kind: "responses", values: ["pw", "123456"] });
  });
});
