import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { usePortForwardingStore } from "../../stores/portForwarding";
import { useTunnelsStore } from "../../stores/tunnels";
import { useUiStore } from "../../stores/ui";
import { makeHost } from "../../test/fixtures";
import { PortForwardingScreen } from "./PortForwardingScreen";

vi.mock("../../ipc/api");

const rule: api.PortForward = { id: "r-1", kind: "local", bind: "8080", target: "localhost:80", autostart: false };
const web = makeHost({ id: "h-web", name: "web", forwards: [rule] });
const db = makeHost({ id: "h-db", name: "db", forwards: [{ ...rule, id: "r-2" }] });
const plain = makeHost({ id: "h-plain", name: "plain" });
const imported = makeHost({ id: "h-cfg", name: "cfg", source: "sshConfig", forwards: [rule] });
const card = (name: string) => screen.getByRole("heading", { name }).closest("li") as HTMLElement;
const reason: api.CoreError = { type: "internal", message: "Netz weg" };

beforeEach(() => {
  useHostsStore.setState({ hosts: [web, plain, imported, db], profiles: [], loaded: true });
  usePortForwardingStore.setState(usePortForwardingStore.getInitialState(), true);
  useConnectStore.setState(useConnectStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useTunnelsStore.setState({ states: {} });
  vi.mocked(api.tunnelsClose).mockResolvedValue(undefined);
  vi.mocked(api.tunnelStart).mockResolvedValue(undefined);
});

describe("PortForwardingScreen", () => {
  it("lists only stored hosts with rules, sorted by name", () => {
    render(<PortForwardingScreen />);
    expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual(["db", "web"]);
    expect(within(card("web")).getByText("Getrennt")).toBeInTheDocument();
    expect(within(card("web")).getByText("127.0.0.1:8080 → localhost:80")).toBeInTheDocument();
    expect(within(card("web")).getByRole("button", { name: "Starten" })).toBeDisabled();
  });

  it("shows the empty text without rules", () => {
    useHostsStore.setState({ hosts: [plain] });
    render(<PortForwardingScreen />);
    expect(screen.getByText("Noch keine Weiterleitungen. Regeln legst du im Host-Editor unter „Verbindung“ an.")).toBeInTheDocument();
  });

  it("connects through the connect flow and edits the host", async () => {
    const start = vi.fn(async () => undefined);
    useConnectStore.setState({ start });
    render(<PortForwardingScreen />);
    await userEvent.click(within(card("web")).getByRole("button", { name: "Verbinden" }));
    expect(start).toHaveBeenCalledWith("h-web", "tunnel");
    await userEvent.click(within(card("web")).getByRole("button", { name: "Host bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host", id: "h-web" } });
  });

  it("shows a running connect attempt", () => {
    useConnectStore.setState({ active: { hostId: "h-web", protocol: "tunnel" } as never });
    render(<PortForwardingScreen />);
    expect(within(card("web")).getByText("Verbindet …")).toBeInTheDocument();
    expect(within(card("web")).getByRole("button", { name: "Verbinden" })).toBeDisabled();
    expect(within(card("db")).getByRole("button", { name: "Verbinden" })).toBeEnabled();
  });

  it("a connected card starts rules and disconnects", async () => {
    usePortForwardingStore.getState().connected("h-web", { sessionId: "s-1", forwards: [rule] });
    render(<PortForwardingScreen />);
    expect(within(card("web")).getByText("Verbunden")).toBeInTheDocument();
    await userEvent.click(within(card("web")).getByRole("button", { name: "Starten" }));
    expect(api.tunnelStart).toHaveBeenCalledWith("s-1", "r-1");
    await userEvent.click(within(card("web")).getByRole("button", { name: "Trennen" }));
    expect(api.tunnelsClose).toHaveBeenCalledWith("s-1");
    expect(within(card("web")).getByText("Getrennt")).toBeInTheDocument();
  });

  it("shows reconnecting and lost", () => {
    usePortForwardingStore.getState().connected("h-web", { sessionId: "s-1", forwards: [rule] });
    usePortForwardingStore.getState().onEvent("h-web", "s-1", { kind: "reconnecting", attempt: 2, maxAttempts: 3, retryInMs: 0, reason });
    render(<PortForwardingScreen />);
    expect(within(card("web")).getByText("Verbindet neu (Versuch 2 von 3) …")).toBeInTheDocument();
    expect(within(card("web")).getByRole("button", { name: "Trennen" })).toBeEnabled();
    expect(within(card("web")).getByRole("button", { name: "Starten" })).toBeDisabled();
    act(() => usePortForwardingStore.getState().onEvent("h-web", "s-1", { kind: "lost", error: reason }));
    expect(within(card("web")).getByRole("alert")).toHaveTextContent("Verbindung verloren: Netz weg");
    expect(within(card("web")).getByRole("button", { name: "Verbinden" })).toBeEnabled();
  });

  it("closes the connection of a host that left the list", () => {
    usePortForwardingStore.getState().connected("h-web", { sessionId: "s-1", forwards: [rule] });
    render(<PortForwardingScreen />);
    act(() => useHostsStore.setState({ hosts: [{ ...web, forwards: [] }, db] }));
    expect(api.tunnelsClose).toHaveBeenCalledWith("s-1");
    expect(usePortForwardingStore.getState().entries["h-web"]).toBeUndefined();
  });
});
