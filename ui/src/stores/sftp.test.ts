import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import type { Entry, Listing, SftpConnected } from "../ipc/api";
import { makeHost, makeSessionInfo } from "../test/fixtures";
import { useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useSftpStore } from "./sftp";

vi.mock("../ipc/api");

const file = (name: string): Entry => ({ name, kind: { type: "file" }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false });
const listing = (path: string, names: string[] = []): Listing => ({ path, entries: names.map(file) });
const connected = (sessionId = "s1", path = "/home/u", names = ["a", "b", "c"]): SftpConnected => ({
  sessionId, info: makeSessionInfo(), version: 3, listing: listing(path, names),
});
const tab = () => useSftpStore.getState().tabs[0]!;

beforeEach(() => {
  vi.mocked(api.localHome).mockResolvedValue("/home/me");
  vi.mocked(api.localList).mockImplementation(async (path) => listing(path, ["l1"]));
  vi.mocked(api.sftpList).mockImplementation(async (_s, path) => listing(path, ["x"]));
  vi.mocked(api.sftpClose).mockResolvedValue();
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useHostsStore.setState({ ...useHostsStore.getInitialState(), hosts: [makeHost({ id: "h1", name: "web" })] }, true);
});

describe("useSftpStore", () => {
  it("opens a tab with the remote listing and loads the local home", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    expect(tab().panes.remote.path).toBe("/home/u");
    expect(tab().title).toBe("web");
    await vi.waitFor(() => expect(tab().panes.local.path).toBe("/home/me"));
    expect(api.localHome).toHaveBeenCalledWith("h1");
    expect(useSftpStore.getState().activeTabId).toBe("t1");
  });

  it("numbers tabs of the same host", () => {
    useSftpStore.getState().openTab("t1", "h1", connected("s1"));
    useSftpStore.getState().openTab("t2", "h1", connected("s2"));
    expect(useSftpStore.getState().tabs.map((x) => x.title)).toEqual(["web", "web 2"]);
  });

  it("keeps the old path and shows the error when navigating fails", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    const denied = { type: "fs", kind: "permissionDenied", path: "/root", message: "denied" } as const;
    vi.mocked(api.sftpList).mockRejectedValueOnce(denied);
    await useSftpStore.getState().navigate("t1", "remote", "/root");
    expect(tab().panes.remote.path).toBe("/home/u");
    expect(tab().panes.remote.error).toEqual(denied);
    expect(tab().panes.remote.entries.map((e) => e.name)).toEqual(["a", "b", "c"]);
  });

  it("a stale listing does not overwrite a newer navigation", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    let releaseSlow!: (l: Listing) => void;
    vi.mocked(api.sftpList)
      .mockImplementationOnce(() => new Promise((resolve) => { releaseSlow = resolve; }))
      .mockImplementationOnce(async (_s, path) => listing(path, ["fast"]));
    const slow = useSftpStore.getState().navigate("t1", "remote", "/slow");
    await useSftpStore.getState().navigate("t1", "remote", "/fast");
    releaseSlow(listing("/slow", ["slow"]));
    await slow;
    expect(tab().panes.remote.path).toBe("/fast");
  });

  it("puts the cursor on the neighbour after deleting", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    vi.mocked(api.sftpDelete).mockResolvedValue();
    vi.mocked(api.sftpList).mockResolvedValueOnce(listing("/home/u", ["a", "c"]));
    useSftpStore.getState().select("t1", "remote", { selection: [], cursor: "b", anchor: "b" });
    await useSftpStore.getState().remove("t1", "remote", ["b"], "permanent");
    expect(api.sftpDelete).toHaveBeenCalledWith("s1", "/home/u", ["b"]);
    expect(tab().panes.remote.cursor).toBe("c");
  });

  it("uses the trash for local deletes in trash mode", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    await vi.waitFor(() => expect(tab().panes.local.path).toBe("/home/me"));
    vi.mocked(api.localTrash).mockResolvedValue();
    await useSftpStore.getState().remove("t1", "local", ["l1"], "trash");
    expect(api.localTrash).toHaveBeenCalledWith("/home/me", ["l1"]);
  });

  it("rename rejects with the core error so the dialog can show it", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    const exists = { type: "fs", kind: "alreadyExists", path: "/home/u/b", message: "x" } as const;
    vi.mocked(api.sftpRename).mockRejectedValueOnce(exists);
    await expect(useSftpStore.getState().rename("t1", "remote", "a", "b")).rejects.toEqual(exists);
  });

  it("marks only the bound session as lost and reconnects into the same tab", async () => {
    const start = vi.fn(async () => undefined);
    useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
    useSftpStore.getState().openTab("t1", "h1", connected("s1"));
    useSftpStore.getState().markLost("t1", "other", { type: "cancelled" });
    expect(tab().status).toBe("connected");
    useSftpStore.getState().markLost("t1", "s1", { type: "network", kind: "other", message: "lost" });
    expect(tab().status).toBe("lost");
    useSftpStore.getState().reconnect("t1");
    expect(start).toHaveBeenCalledWith("h1", "sftp", { sftpTabId: "t1" });
    await useSftpStore.getState().navigate("t1", "remote", "/home/u/deep");
    expect(useSftpStore.getState().attachSession("t1", connected("s2", "/home/u"))).toBe(true);
    expect(tab().status).toBe("connected");
    expect(tab().sessionId).toBe("s2");
    await vi.waitFor(() => expect(api.sftpList).toHaveBeenLastCalledWith("s2", "/home/u/deep"));
  });

  it("reconnect into a closed tab closes the new session", () => {
    expect(useSftpStore.getState().attachSession("gone", connected("s9"))).toBe(false);
  });

  it("closing a tab closes its session", () => {
    useSftpStore.getState().openTab("t1", "h1", connected("s1"));
    useSftpStore.getState().closeTab("t1");
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
    expect(useSftpStore.getState().tabs).toEqual([]);
  });
});
