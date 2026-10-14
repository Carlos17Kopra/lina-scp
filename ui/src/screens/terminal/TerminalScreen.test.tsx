import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConnectStore } from "../../stores/connect";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { useUiStore } from "../../stores/ui";
import { TerminalScreen } from "./TerminalScreen";

vi.mock("../../ipc/api");
vi.mock("./TerminalView", () => ({
  TerminalView: ({ tab, visible }: { tab: Tab; visible: boolean }) => (
    <div data-testid={`term-${tab.id}`} data-visible={String(visible)} />
  ),
}));

const openLocal = vi.fn(async () => undefined);
const closeSession = vi.fn(async () => undefined);
const start = vi.fn(async () => undefined);
const web: Tab = { id: "a", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s-a", status: "connected" };
const local: Tab = { id: "b", title: "Lokales Terminal", kind: "local", sessionId: "s-b", status: "connected" };

function seed(tabs: Tab[], activeId: string | null) {
  useSessionsStore.setState({ tabs, activeId, openLocal, closeSession });
}

beforeEach(() => {
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useUiStore.setState({ ...useUiStore.getInitialState(), view: "terminal" }, true);
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
});

describe("TerminalScreen", () => {
  it("shows an empty state that opens a local terminal", async () => {
    seed([], null);
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.getByText("Keine offenen Sitzungen")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Lokales Terminal öffnen" }));
    expect(openLocal).toHaveBeenCalledOnce();
  });

  it("renders one tab and panel per session; only the active one is visible", async () => {
    seed([web, local], "a");
    render(<TerminalScreen />);
    const tablist = screen.getByRole("tablist", { name: "Sitzungen" });
    expect(within(tablist).getByRole("tab", { name: "web-prod-01" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("term-a")).toHaveAttribute("data-visible", "true");
    expect(screen.getByTestId("term-b")).toHaveAttribute("data-visible", "false");
    expect(screen.getByTestId("term-b").closest("[role=tabpanel]")).not.toBeVisible();
    await userEvent.click(within(tablist).getByRole("tab", { name: "Lokales Terminal" }));
    expect(useSessionsStore.getState().activeId).toBe("b");
    expect(screen.getByTestId("term-b")).toHaveAttribute("data-visible", "true");
  });

  it("hides every terminal while another view is shown", () => {
    seed([web], "a");
    useUiStore.setState({ view: "hosts" });
    render(<TerminalScreen />);
    expect(screen.getByTestId("term-a")).toHaveAttribute("data-visible", "false");
  });

  it("opens local terminals and closes tabs from the tab bar", async () => {
    seed([web], "a");
    render(<TerminalScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neues lokales Terminal" }));
    expect(openLocal).toHaveBeenCalledOnce();
    await userEvent.click(screen.getByRole("button", { name: "web-prod-01 schließen" }));
    expect(closeSession).toHaveBeenCalledWith("a");
  });

  it("offers close and reconnect when an SSH session ended", async () => {
    seed([{ ...web, status: "closed" }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung beendet" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(closeSession).toHaveBeenCalledWith("a");
    expect(start).toHaveBeenCalledWith("h-web", "ssh");
  });

  it("offers only closing for ended local sessions", async () => {
    seed([{ ...local, status: "error" }], "b");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung mit Fehler beendet" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Neu verbinden" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(closeSession).toHaveBeenCalledWith("b");
  });
});
