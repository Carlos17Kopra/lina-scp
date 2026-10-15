import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useSftpStore } from "../../stores/sftp";
import { makeHost, makeSessionInfo } from "../../test/fixtures";
import { SftpScreen } from "./SftpScreen";

vi.mock("../../ipc/api");

const start = vi.fn(async () => undefined);

beforeEach(() => {
  start.mockClear();
  vi.mocked(api.localHome).mockResolvedValue("/l");
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [] });
  vi.mocked(api.sftpClose).mockResolvedValue();
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useHostsStore.setState({
    ...useHostsStore.getInitialState(),
    hosts: [
      makeHost({ id: "h1", name: "web", user: "deploy", address: "web.example" }),
      makeHost({ id: "h2", name: "ssh-only", protocolSftp: false }),
    ],
  }, true);
});

const openTab = () =>
  useSftpStore.getState().openTab("t1", "h1", {
    sessionId: "s1", info: makeSessionInfo({ cipher: "chacha20-poly1305@openssh.com" }), version: 3,
    listing: { path: "/srv", entries: [] },
  });

describe("SftpScreen", () => {
  it("offers only SFTP hosts when no session is open", async () => {
    render(<SftpScreen />);
    expect(screen.queryByText("ssh-only")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /web/ }));
    expect(start).toHaveBeenCalledWith("h1", "sftp");
  });

  it("shows both panes and the status line", () => {
    openTab();
    render(<SftpScreen />);
    expect(screen.getByRole("grid", { name: "Lokal" })).toBeInTheDocument();
    expect(screen.getByRole("grid", { name: "Remote" })).toBeInTheDocument();
    expect(screen.getByText("deploy@web.example · SFTP v3 · chacha20-poly1305@openssh.com")).toBeInTheDocument();
  });

  it("shows the lost banner and reconnects", async () => {
    openTab();
    useSftpStore.getState().markLost("t1", "s1", { type: "network", kind: "other", message: "connection lost" });
    render(<SftpScreen />);
    expect(screen.getByText("Verbindung verloren")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(start).toHaveBeenCalledWith("h1", "sftp", { sftpTabId: "t1" });
  });

  it("opens the new-folder dialog from the toolbar and creates the folder", async () => {
    openTab();
    vi.mocked(api.sftpMkdir).mockResolvedValue();
    vi.mocked(api.sftpList).mockResolvedValue({ path: "/srv", entries: [] });
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neuer Ordner" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "neu{Enter}");
    expect(api.sftpMkdir).toHaveBeenCalledWith("s1", "/srv", "neu");
  });

  it("opens a terminal to the same host", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(start).toHaveBeenCalledWith("h1", "ssh");
  });

  it("closes a tab", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "web schließen" }));
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
  });
});
