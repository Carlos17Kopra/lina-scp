import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import type { Bookmark } from "../ipc/api";
import { useBookmarksStore } from "./bookmarks";
import { useNoticeStore } from "./notices";

vi.mock("../ipc/api");

const bm = (id: string, path: string, hostId: string | null = "h1"): Bookmark => ({ id, hostId, path, position: 1, createdAt: 0 });

beforeEach(() => {
  useBookmarksStore.setState(useBookmarksStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
});

describe("bookmarks store", () => {
  it("loads the local and the host bookmarks under their key", async () => {
    vi.mocked(api.bookmarksList).mockImplementation(async (hostId) => (hostId ? [bm("b1", "/srv")] : [bm("b2", "/home", null)]));
    await useBookmarksStore.getState().load("h1");
    await useBookmarksStore.getState().load("local");
    expect(api.bookmarksList).toHaveBeenCalledWith(null);
    expect(useBookmarksStore.getState().byKey).toEqual({ h1: [bm("b1", "/srv")], local: [bm("b2", "/home", null)] });
  });

  it("toggle adds a missing bookmark and removes an existing one", async () => {
    vi.mocked(api.bookmarkAdd).mockResolvedValue(bm("b1", "/srv"));
    vi.mocked(api.bookmarkRemove).mockResolvedValue();
    vi.mocked(api.bookmarksList).mockResolvedValueOnce([bm("b1", "/srv")]).mockResolvedValueOnce([]);
    await useBookmarksStore.getState().toggle("h1", "/srv");
    expect(api.bookmarkAdd).toHaveBeenCalledWith("h1", "/srv");
    expect(useBookmarksStore.getState().byKey.h1).toEqual([bm("b1", "/srv")]);
    await useBookmarksStore.getState().toggle("h1", "/srv");
    expect(api.bookmarkRemove).toHaveBeenCalledWith("b1");
    expect(useBookmarksStore.getState().byKey.h1).toEqual([]);
  });

  it("reports failures as a notice instead of rejecting", async () => {
    vi.mocked(api.bookmarkAdd).mockRejectedValue({ type: "config", message: "bad" });
    await useBookmarksStore.getState().toggle("local", "/x");
    expect(useNoticeStore.getState().notices[0]?.message).toBe("Lesezeichen konnte nicht gespeichert werden");
  });
});
