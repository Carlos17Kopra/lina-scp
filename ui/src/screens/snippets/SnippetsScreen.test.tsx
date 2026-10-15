import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import { makeHost, makeSnippet } from "../../test/fixtures";
import { SnippetsScreen } from "./SnippetsScreen";

vi.mock("../../ipc/api");

const rowNames = () =>
  screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent);

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState({ ...useHostsStore.getInitialState(), hosts: [makeHost({ id: "h1", name: "web" })] }, true);
  useSnippetsStore.setState({
    snippets: [
      makeSnippet({ id: "a", name: "Deploy", command: "git pull\nmake", hostId: "h1" }),
      makeSnippet({ id: "b", name: "Speicher", command: "df -h", hostId: null }),
    ],
    loaded: true,
  });
});

describe("SnippetsScreen", () => {
  it("lists all snippets with scope and first command line", () => {
    render(<SnippetsScreen />);
    expect(rowNames()).toEqual(["Deploy", "Speicher"]);
    const deploy = screen.getAllByRole("row")[1] as HTMLElement;
    expect(within(deploy).getByText("web")).toBeInTheDocument();
    expect(within(deploy).getByText("git pull")).toBeInTheDocument();
  });

  it("filters by scope and search", async () => {
    render(<SnippetsScreen />);
    await userEvent.click(screen.getByRole("radio", { name: "Global" }));
    expect(rowNames()).toEqual(["Speicher"]);
    await userEvent.click(screen.getByRole("radio", { name: "Host" }));
    expect(rowNames()).toEqual(["Deploy"]);
    await userEvent.click(screen.getByRole("radio", { name: "Alle" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Snippets suchen" }), "df");
    expect(rowNames()).toEqual(["Speicher"]);
  });

  it("opens the editor for a row and for a new global snippet", async () => {
    render(<SnippetsScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Deploy" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", id: "a" });
    await userEvent.click(screen.getByRole("button", { name: "+ Snippet anlegen" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", hostId: null });
  });

  it("shows an empty state", () => {
    useSnippetsStore.setState({ snippets: [] });
    render(<SnippetsScreen />);
    expect(screen.getByText("Noch keine Snippets")).toBeInTheDocument();
  });
});
