import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../../ipc/api";
import type { PortForward } from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { sessionEventHandler } from "../../../stores/sessionEvents";
import { useTunnelsStore } from "../../../stores/tunnels";
import { useUiStore } from "../../../stores/ui";
import { makeHost } from "../../../test/fixtures";
import { TunnelPanel } from "./TunnelPanel";

vi.mock("../../../ipc/api");

const local: PortForward = { id: "r1", kind: "local", bind: "8080", target: "localhost:80", autostart: false };
const socks: PortForward = { id: "r2", kind: "dynamic", bind: "[::1]:1080", target: "", autostart: true };
const remote: PortForward = { id: "r3", kind: "remote", bind: "0", target: "127.0.0.1:3000", autostart: false };
const tab: Tab = { id: "t1", title: "web", kind: "ssh", hostId: "h-web", sessionId: "s1", status: "connected", forwards: [local, socks, remote] };

const row = (text: string) => screen.getByText(text).closest("li") as HTMLElement;

beforeEach(() => {
  useTunnelsStore.setState(useTunnelsStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState({ hosts: [makeHost({ id: "h-web" })] });
  vi.mocked(api.tunnelStart).mockResolvedValue();
  vi.mocked(api.tunnelStop).mockResolvedValue();
});

describe("TunnelPanel", () => {
  it("lists the rules with their state and starts and stops them", async () => {
    const send = sessionEventHandler("t1");
    send({ kind: "tunnel", event: { ruleId: "r2", state: { kind: "active", port: 1080 } } });
    send({ kind: "tunnel", event: { ruleId: "r3", state: { kind: "failed", error: { type: "network", kind: "other", message: "verweigert" } } } });
    render(<TunnelPanel tab={tab} />);
    expect(within(row("127.0.0.1:8080 → localhost:80")).getByText("Aus")).toBeInTheDocument();
    expect(within(row("[::1]:1080 · SOCKS")).getByText("Aktiv")).toBeInTheDocument();
    expect(within(row("localhost:0 → 127.0.0.1:3000")).getByRole("alert")).toHaveTextContent("Fehler: verweigert");

    await userEvent.click(within(row("127.0.0.1:8080 → localhost:80")).getByRole("button", { name: "Starten" }));
    expect(api.tunnelStart).toHaveBeenCalledWith("s1", "r1");
    await userEvent.click(within(row("[::1]:1080 · SOCKS")).getByRole("button", { name: "Stoppen" }));
    expect(api.tunnelStop).toHaveBeenCalledWith("s1", "r2");
    await userEvent.click(within(row("localhost:0 → 127.0.0.1:3000")).getByRole("button", { name: "Starten" }));
    expect(api.tunnelStart).toHaveBeenCalledWith("s1", "r3");
  });

  it("shows the port the server picked and disables the switches while not connected", () => {
    sessionEventHandler("t1")({ kind: "tunnel", event: { ruleId: "r3", state: { kind: "active", port: 41234 } } });
    render(<TunnelPanel tab={{ ...tab, status: "reconnecting" }} />);
    expect(within(row("localhost:0 → 127.0.0.1:3000")).getByText("Aktiv · Port 41234")).toBeInTheDocument();
    for (const button of screen.getAllByRole("button", { name: /Starten|Stoppen/ })) expect(button).toBeDisabled();
  });

  it("says when there are no rules and links to the host editor", async () => {
    render(<TunnelPanel tab={{ ...tab, forwards: [] }} />);
    expect(screen.getByText("Für diesen Host sind keine Regeln angelegt.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Host bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host", id: "h-web" } });
  });

  it("has no editor link for local terminals and hosts from ~/.ssh/config", () => {
    const { unmount } = render(<TunnelPanel tab={{ id: "t2", title: "Lokal", kind: "local", status: "connected" }} />);
    expect(screen.getByText("Für diesen Host sind keine Regeln angelegt.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Host bearbeiten" })).not.toBeInTheDocument();
    unmount();
    useHostsStore.setState({ hosts: [makeHost({ id: "h-web", source: "sshConfig" })] });
    render(<TunnelPanel tab={{ ...tab, forwards: [] }} />);
    expect(screen.queryByRole("button", { name: "Host bearbeiten" })).not.toBeInTheDocument();
  });
});
