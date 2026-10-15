import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { useUiStore } from "../../stores/ui";
import { singleGroups } from "../../test/fixtures";
import { TerminalScreen } from "./TerminalScreen";

vi.mock("../../ipc/api");
vi.mock("./TerminalView", () => ({
  TerminalView: ({ tab, visible }: { tab: Tab; visible: boolean }) => (
    <div data-testid={`term-${tab.id}`} data-visible={String(visible)} />
  ),
}));

const openLocal = vi.fn(async () => undefined);
const closeSession = vi.fn(async () => undefined);
const closeGroup = vi.fn(async () => undefined);
const start = vi.fn(async () => undefined);
const web: Tab = { id: "a", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s-a", status: "connected" };
const local: Tab = { id: "b", title: "Lokales Terminal", kind: "local", sessionId: "s-b", status: "connected" };

function seed(tabs: Tab[], activeId: string | null) {
  useSessionsStore.setState({ tabs, groups: singleGroups(tabs), activeId, openLocal, closeSession, closeGroup });
}

beforeEach(() => {
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useUiStore.setState({ ...useUiStore.getInitialState(), view: "terminal" }, true);
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  useSettingsStore.setState({ settings: DEFAULT_SETTINGS, loaded: true });
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

  it("toggles the sidebar from the tab bar, remembers it and shows the active tab's session", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
    seed([web, local], "a");
    render(
      <ToastProvider>
        <TerminalScreen />
      </ToastProvider>,
    );
    expect(screen.queryByRole("complementary", { name: "Sitzungsdetails" })).toBeNull();
    const toggle = screen.getByRole("button", { name: "Seitenleiste ein-/ausblenden (Ctrl+Shift+B)" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(toggle);
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalSidebarOpen: true }));
    const sidebar = screen.getByRole("complementary", { name: "Sitzungsdetails" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(within(sidebar).getByRole("tab", { name: "Sitzung" })).toHaveAttribute("aria-selected", "true");
    expect(within(sidebar).getByRole("tab", { name: "Tunnel" })).toBeDisabled();
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Snippets" }));
    expect(within(sidebar).getByRole("searchbox", { name: "Snippets suchen" })).toBeInTheDocument();
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Sitzung" }));
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Design" }));
    expect(within(sidebar).getByRole("radiogroup", { name: "Farbschema" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Lokales Terminal" }));
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Sitzung" }));
    expect(within(sidebar).getByText("Lokale Shell")).toBeInTheDocument();
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

  it("offers close and reconnect in the same pane when an SSH session ended", async () => {
    vi.mocked(api.newId).mockReturnValue("p-new");
    seed([{ ...web, status: "closed" }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung beendet" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(api.sessionClose).toHaveBeenCalledWith("s-a");
    expect(start).toHaveBeenCalledWith("h-web", "ssh", { paneId: "p-new" });
    expect(useSessionsStore.getState().groups[0]).toMatchObject({ id: "a", focusedPaneId: "p-new" });
  });

  it("shows the error of a session that ended with an error", () => {
    seed([{ ...web, status: "error", error: { type: "io", message: "shell request refused" } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Sitzung mit Fehler beendet" })).toBeInTheDocument();
    expect(screen.getByText("shell request refused")).toBeInTheDocument();
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
