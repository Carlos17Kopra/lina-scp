import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Bookmark } from "../../ipc/api";
import { useBookmarksStore } from "../../stores/bookmarks";
import { BookmarkMenu } from "./BookmarkMenu";

vi.mock("../../ipc/api");

const bm = (id: string, path: string): Bookmark => ({ id, hostId: "h1", path, position: 1, createdAt: 0 });
const onNavigate = vi.fn();

beforeEach(() => {
  onNavigate.mockClear();
  useBookmarksStore.setState(useBookmarksStore.getInitialState(), true);
  vi.mocked(api.bookmarksList).mockResolvedValue([]);
});

const renderMenu = (path = "/srv") => render(<BookmarkMenu bookmarkKey="h1" path={path} onNavigate={onNavigate} />);

describe("BookmarkMenu", () => {
  it("opens an empty menu and bookmarks the current folder", async () => {
    vi.mocked(api.bookmarkAdd).mockResolvedValue(bm("b1", "/srv"));
    renderMenu();
    await userEvent.click(screen.getByRole("button", { name: "Lesezeichen" }));
    expect(await screen.findByText("Noch keine Lesezeichen")).toBeInTheDocument();
    vi.mocked(api.bookmarksList).mockResolvedValue([bm("b1", "/srv")]);
    await userEvent.click(screen.getByRole("menuitem", { name: "Diesen Ordner merken" }));
    expect(api.bookmarkAdd).toHaveBeenCalledWith("h1", "/srv");
    expect(screen.getByRole("button", { name: "Lesezeichen" })).toHaveAttribute("aria-pressed", "true");
  });

  it("offers removal for a bookmarked folder", async () => {
    useBookmarksStore.setState({ byKey: { h1: [bm("b1", "/srv")] } });
    vi.mocked(api.bookmarksList).mockResolvedValue([bm("b1", "/srv")]);
    vi.mocked(api.bookmarkRemove).mockResolvedValue();
    renderMenu();
    await userEvent.click(screen.getByRole("button", { name: "Lesezeichen" }));
    vi.mocked(api.bookmarksList).mockResolvedValue([]);
    await userEvent.click(screen.getByRole("menuitem", { name: "Lesezeichen entfernen" }));
    expect(api.bookmarkRemove).toHaveBeenCalledWith("b1");
  });

  it("navigates to a bookmark and removes single rows", async () => {
    vi.mocked(api.bookmarksList).mockResolvedValue([bm("b1", "/etc/nginx"), bm("b2", "/")]);
    vi.mocked(api.bookmarkRemove).mockResolvedValue();
    renderMenu();
    await userEvent.click(screen.getByRole("button", { name: "Lesezeichen" }));
    const item = await screen.findByRole("menuitem", { name: "nginx" });
    expect(item).toHaveAttribute("title", "/etc/nginx");
    expect(screen.getByRole("menuitem", { name: "/" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "/ entfernen" }));
    expect(api.bookmarkRemove).toHaveBeenCalledWith("b2");
    await userEvent.click(item);
    expect(onNavigate).toHaveBeenCalledWith("/etc/nginx");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("moves focus with the arrow keys and closes with Escape", async () => {
    vi.mocked(api.bookmarksList).mockResolvedValue([bm("b1", "/etc")]);
    renderMenu();
    const button = screen.getByRole("button", { name: "Lesezeichen" });
    await userEvent.click(button);
    await screen.findByRole("menuitem", { name: "etc" });
    expect(screen.getByRole("menuitem", { name: "Diesen Ordner merken" })).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "etc" })).toHaveFocus();
    await userEvent.keyboard("{ArrowUp}");
    expect(screen.getByRole("menuitem", { name: "Diesen Ordner merken" })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(button).toHaveFocus();
  });
});
