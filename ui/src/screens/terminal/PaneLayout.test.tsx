import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { PaneLayout } from "./PaneLayout";

vi.mock("../../ipc/api");
const mounts: string[] = [];
vi.mock("./TerminalView", async () => {
  const { useEffect } = await import("react");
  return {
    TerminalView: ({ tab, focused }: { tab: Tab; focused?: boolean }) => {
      useEffect(() => {
        mounts.push(tab.id);
      }, [tab.id]);
      return <div data-testid={`term-${tab.id}`} data-focused={String(!!focused)} />;
    },
  };
});

const a: Tab = { id: "a", title: "web", kind: "ssh", hostId: "h", sessionId: "s-a", status: "connected" };

beforeEach(() => {
  mounts.length = 0;
  vi.mocked(api.newId).mockReturnValue("p-2");
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useSessionsStore.getState().addTab(a);
});

const group = () => useSessionsStore.getState().groups[0]!;

describe("PaneLayout", () => {
  it("splits without remounting the running terminal (Review Focus 1)", () => {
    const { rerender } = render(<PaneLayout group={group()} visible />);
    act(() => useSessionsStore.getState().split("row"));
    rerender(<PaneLayout group={group()} visible />);
    expect(mounts).toEqual(["a"]);
    expect(screen.getByRole("combobox", { name: "Host suchen" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Bereich schließen" })).toHaveLength(2);
  });

  it("focuses a pane on mouse down and moves the divider with the keyboard", () => {
    act(() => useSessionsStore.getState().split("row"));
    render(<PaneLayout group={group()} visible />);
    fireEvent.mouseDown(screen.getByTestId("term-a"));
    expect(group().focusedPaneId).toBe("a");
    const separator = screen.getByRole("separator", { name: "Bereichsgröße" });
    expect(separator).toHaveAttribute("aria-orientation", "vertical");
    expect(separator).toHaveAttribute("aria-valuenow", "50");
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(group().layout).toMatchObject({ ratio: 0.55 });
  });

  it("shows a single pane without frame controls", () => {
    render(<PaneLayout group={group()} visible />);
    expect(screen.queryByRole("button", { name: "Bereich schließen" })).toBeNull();
    expect(screen.queryByRole("separator")).toBeNull();
    expect(screen.getByTestId("term-a")).toHaveAttribute("data-focused", "true");
  });

  it("shows the reconnect bar over a reconnecting pane and keeps its terminal", () => {
    const reason = { type: "network", kind: "other", message: "x" } as const;
    act(() =>
      useSessionsStore.getState().updateTab("a", {
        status: "reconnecting",
        reconnect: { attempt: 1, maxAttempts: 8, retryAt: Date.now() + 1000, reason },
      }),
    );
    render(<PaneLayout group={group()} visible />);
    expect(screen.getByRole("status")).toHaveTextContent("Versuch 1 von 8");
    expect(screen.getByTestId("term-a")).toBeInTheDocument();
  });
});
