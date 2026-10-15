import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useSnippetsStore } from "../../../stores/snippets";
import { useUiStore } from "../../../stores/ui";
import { makeHost, makeSnippet } from "../../../test/fixtures";
import { dropPasteTarget, registerPasteTarget } from "../pasteTargets";
import { SnippetsPanel } from "./SnippetsPanel";

vi.mock("../../../ipc/api");

const sshTab: Tab = { id: "t1", title: "web", kind: "ssh", hostId: "h1", sessionId: "s1", status: "connected" };
const localTab: Tab = { id: "t1", title: "Lokal", kind: "local", sessionId: "s2", status: "connected" };
const target = { insert: vi.fn(), run: vi.fn() };

// Core order (by name).
const SNIPPETS = [
  makeSnippet({ id: "a", name: "Fremd", command: "uptime", hostId: "h9" }),
  makeSnippet({ id: "b", name: "Logs", command: "journalctl -u {{unit}} -n {{lines}} # {{unit}}", hostId: null }),
  makeSnippet({ id: "c", name: "Nginx", command: "sudo nginx -t &&\nsudo systemctl reload nginx", hostId: "h1" }),
  makeSnippet({ id: "d", name: "Speicher", command: "df -h", hostId: null }),
];

const cardNames = () =>
  screen.getAllByRole("button", { name: / einfügen$/ }).map((button) => button.getAttribute("aria-label"));
const card = (name: string) => screen.getByRole("button", { name: `${name} einfügen` }).closest("li") as HTMLElement;

beforeEach(() => {
  target.insert.mockClear();
  target.run.mockClear();
  registerPasteTarget("t1", target);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState({ ...useHostsStore.getInitialState(), hosts: [makeHost({ id: "h1", name: "web" })] }, true);
  useSnippetsStore.setState({ snippets: SNIPPETS, loaded: true });
});

afterEach(() => dropPasteTarget("t1"));

describe("SnippetsPanel", () => {
  it("lists the host's snippets first, then global ones, and filters by name and command", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    expect(cardNames()).toEqual(["Nginx einfügen", "Logs einfügen", "Speicher einfügen"]);
    expect(within(card("Nginx")).getByText("web")).toBeInTheDocument();
    expect(within(card("Speicher")).getByText("Global")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("searchbox", { name: "Snippets suchen" }), "DF");
    expect(cardNames()).toEqual(["Speicher einfügen"]);
    await userEvent.type(screen.getByRole("searchbox", { name: "Snippets suchen" }), "zzz");
    expect(screen.getByText("Keine Treffer")).toBeInTheDocument();
  });

  it("shows only global snippets in a local terminal", () => {
    render(<SnippetsPanel tab={localTab} />);
    expect(cardNames()).toEqual(["Logs einfügen", "Speicher einfügen"]);
  });

  it("shows an empty state", () => {
    useSnippetsStore.setState({ snippets: [] });
    render(<SnippetsPanel tab={sshTab} />);
    expect(screen.getByText("Noch keine Snippets")).toBeInTheDocument();
  });

  it("inserts on click and runs with the play button", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "Speicher einfügen" }));
    expect(target.insert).toHaveBeenCalledWith("df -h");
    await userEvent.click(within(card("Nginx")).getByRole("button", { name: "Ausführen" }));
    expect(target.run).toHaveBeenCalledWith("sudo nginx -t &&\nsudo systemctl reload nginx");
  });

  it("asks for placeholder values first; cancel sends nothing", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "Logs einfügen" }));
    const dialog = screen.getByRole("dialog", { name: "Platzhalter ausfüllen" });
    expect(within(dialog).getAllByRole("textbox").map((box) => box.getAttribute("aria-label") ?? "")).toHaveLength(2);
    await userEvent.click(within(dialog).getByRole("button", { name: "Abbrechen" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(target.insert).not.toHaveBeenCalled();

    await userEvent.click(within(card("Logs")).getByRole("button", { name: "Ausführen" }));
    const again = screen.getByRole("dialog", { name: "Platzhalter ausfüllen" });
    await userEvent.type(within(again).getByRole("textbox", { name: "unit" }), "nginx");
    await userEvent.type(within(again).getByRole("textbox", { name: "lines" }), "50{Enter}");
    // Review Focus 3: sent only after the dialog (and its focus trap) is gone.
    await waitFor(() => expect(target.run).toHaveBeenCalledWith("journalctl -u nginx -n 50 # nginx"));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("disables insert and run while the tab is not connected", () => {
    render(<SnippetsPanel tab={{ ...sshTab, status: "closed" }} />);
    expect(screen.getByRole("button", { name: "Speicher einfügen" })).toBeDisabled();
    expect(within(card("Speicher")).getByRole("button", { name: "Ausführen" })).toBeDisabled();
  });

  it("opens the editor for new and existing snippets", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "+ Snippet anlegen" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", hostId: "h1" });
    await userEvent.click(within(card("Nginx")).getByRole("button", { name: "Bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", id: "c" });
  });
});
