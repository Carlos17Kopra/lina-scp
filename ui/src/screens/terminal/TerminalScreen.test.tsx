import { act, render, screen, within } from "@testing-library/react";
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
    expect(within(sidebar).getByRole("tab", { name: "Tunnel" })).toBeEnabled();
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
    expect(closeGroup).toHaveBeenCalledWith("a");
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

  it("shows the focused pane in the sidebar; a picking pane has no terminal", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalSidebarOpen: true } });
    seed([web], "a");
    vi.mocked(api.newId).mockReturnValue("p-2");
    act(() => useSessionsStore.getState().split("row"));
    render(<TerminalScreen />);
    const sidebar = screen.getByRole("complementary", { name: "Sitzungsdetails" });
    expect(within(sidebar).getByText("Kein Terminal")).toBeInTheDocument();
  });

  it("splits, counts panes, limits to 4 and toggles broadcast from the tab bar", async () => {
    seed([web], "a");
    let next = 0;
    vi.mocked(api.newId).mockImplementation(() => `p-${++next}`);
    render(<TerminalScreen />);
    const broadcast = screen.getByRole("button", { name: "Broadcast" });
    expect(broadcast).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Nebeneinander teilen" }));
    expect(screen.getByRole("tab", { name: "web-prod-01 · 2" })).toBeInTheDocument();
    await userEvent.click(broadcast);
    expect(broadcast).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Broadcast an · 2 Bereiche")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Untereinander teilen" }));
    await userEvent.click(screen.getByRole("button", { name: "Untereinander teilen" }));
    expect(screen.getByRole("button", { name: "Nebeneinander teilen" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Untereinander teilen" })).toBeDisabled();
  });

  it("offers retry, reconnect and close when the connection was lost", async () => {
    vi.mocked(api.sessionReconnectNow).mockResolvedValue(undefined);
    seed([{ ...web, status: "lost", error: { type: "network", kind: "timeout", message: "timed out" } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Verbindung verloren" })).toBeInTheDocument();
    expect(screen.getByText("timed out")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /web-prod-01\s*– Verbindung verloren/ })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Erneut versuchen" }));
    expect(api.sessionReconnectNow).toHaveBeenCalledWith("s-a");
    expect(screen.getByRole("button", { name: "Neu verbinden" })).toBeInTheDocument();
  });

  it("only offers an interactive reconnect when input is required", () => {
    seed([{ ...web, status: "lost", error: { type: "inputRequired" } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByText("Für die Verbindung ist eine Eingabe nötig (Passwort, Host-Key …).")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
    expect(screen.getByRole("button", { name: "Neu verbinden" })).toBeInTheDocument();
  });

  it("only offers close when the host is gone", () => {
    seed([{ ...web, status: "lost", error: { type: "config", message: "host h-web not found" } }], "a");
    render(<TerminalScreen />);
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Neu verbinden" })).toBeNull();
    expect(screen.getByRole("button", { name: "Schließen" })).toBeInTheDocument();
  });

  it("marks a reconnecting tab for screen readers", () => {
    const reason = { type: "network", kind: "other", message: "x" } as const;
    seed([{ ...web, status: "reconnecting", reconnect: { attempt: 1, maxAttempts: 8, retryAt: Date.now() + 1000, reason } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("tab", { name: /web-prod-01\s*– Verbindung unterbrochen/ })).toBeInTheDocument();
  });

  it("offers no silent retry for a rejected host key", () => {
    seed([{ ...web, status: "lost", error: { type: "hostKeyRejected", message: "revoked" } }], "a");
    render(<TerminalScreen />);
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
    expect(screen.getByRole("button", { name: "Neu verbinden" })).toBeInTheDocument();
  });
});
