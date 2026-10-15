import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import { makeHost, makeSnippet } from "../../test/fixtures";
import { SnippetEditor } from "./SnippetEditor";

vi.mock("../../ipc/api");

function renderEditor(props: { id?: string; hostId: string | null }) {
  return render(
    <ToastProvider>
      <SnippetEditor {...props} />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState({ ...useUiStore.getInitialState(), modal: { type: "snippet" } }, true);
  useHostsStore.setState(
    {
      ...useHostsStore.getInitialState(),
      hosts: [
        makeHost({ id: "h1", name: "web-prod-01" }),
        makeHost({ id: "h2", name: "from-config", source: "sshConfig" }),
      ],
    },
    true,
  );
  useSnippetsStore.setState(useSnippetsStore.getInitialState(), true);
  vi.mocked(api.newId).mockReturnValue("sn-new");
  vi.mocked(api.snippetSave).mockImplementation(async (snippet) => snippet);
  vi.mocked(api.snippetsList).mockResolvedValue([]);
});

describe("SnippetEditor", () => {
  it("creates a snippet for the preselected host and closes", async () => {
    renderEditor({ hostId: "h1" });
    const scope = screen.getByRole("combobox", { name: "Gilt für" });
    expect(scope).toHaveValue("h1");
    expect(screen.queryByRole("option", { name: "from-config" })).toBeNull();
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "Logs");
    await userEvent.type(screen.getByRole("textbox", { name: "Befehl" }), "journalctl -u nginx");
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(api.snippetSave).toHaveBeenCalledWith(
      expect.objectContaining({ id: "sn-new", name: "Logs", command: "journalctl -u nginx", hostId: "h1" }),
    );
    expect(useUiStore.getState().modal).toBeNull();
  });

  it("falls back to Global for a host from ~/.ssh/config", () => {
    renderEditor({ hostId: "h2" });
    expect(screen.getByRole("combobox", { name: "Gilt für" })).toHaveValue("global");
  });

  it("requires name and command", async () => {
    renderEditor({ hostId: null });
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(screen.getByText("Name fehlt")).toBeInTheDocument();
    expect(screen.getByText("Befehl fehlt")).toBeInTheDocument();
    expect(api.snippetSave).not.toHaveBeenCalled();
  });

  it("keeps the dialog open when the core rejects the snippet", async () => {
    vi.mocked(api.snippetSave).mockRejectedValue({ type: "config", message: "Befehl darf keine NUL-Zeichen enthalten" });
    renderEditor({ hostId: null });
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "x");
    await userEvent.type(screen.getByRole("textbox", { name: "Befehl" }), "ls");
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(await screen.findByText("Snippet konnte nicht gespeichert werden")).toBeInTheDocument();
    expect(useUiStore.getState().modal).toEqual({ type: "snippet" });
  });

  it("edits an existing snippet and deletes it only after confirmation", async () => {
    useSnippetsStore.setState({ snippets: [makeSnippet({ id: "sn-1", hostId: null })], loaded: true });
    renderEditor({ id: "sn-1", hostId: null });
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Logs folgen");
    expect(screen.getByRole("combobox", { name: "Gilt für" })).toHaveValue("global");
    await userEvent.click(screen.getByRole("button", { name: "Löschen" }));
    expect(screen.getByText("Snippet wirklich löschen?")).toBeInTheDocument();
    expect(api.snippetDelete).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Löschen" }));
    expect(api.snippetDelete).toHaveBeenCalledWith("sn-1");
    expect(useUiStore.getState().modal).toBeNull();
  });
});
