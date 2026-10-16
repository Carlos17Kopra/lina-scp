import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useEditsStore } from "../../stores/edits";
import { useSftpStore } from "../../stores/sftp";
import { EditConflictDialog } from "./EditConflictDialog";

vi.mock("../../ipc/api");

beforeEach(() => {
  useEditsStore.setState(useEditsStore.getInitialState(), true);
  useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
  vi.mocked(api.editResolve).mockResolvedValue();
  useEditsStore.getState().onEvent("t", {
    kind: "file", file: { id: 4, name: "nginx.conf", remotePath: "/etc/nginx.conf", state: "conflict", dirty: true, error: null },
  });
});

describe("EditConflictDialog", () => {
  it("asks with focus on keeping the server's version; Escape keeps it", async () => {
    render(<EditConflictDialog tabId="t" />);
    expect(screen.getByRole("dialog", { name: "Datei auf dem Server geändert" })).toBeInTheDocument();
    expect(screen.getByText("nginx.conf wurde auf dem Server geändert, seit sie geöffnet wurde. Mit deiner Version überschreiben?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Nicht hochladen" })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(api.editResolve).toHaveBeenCalledWith("s1", 4, false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("overwrites on request", async () => {
    render(<EditConflictDialog tabId="t" />);
    await userEvent.click(screen.getByRole("button", { name: "Überschreiben" }));
    expect(api.editResolve).toHaveBeenCalledWith("s1", 4, true);
  });

  it("shows nothing for a tab without conflict", () => {
    const { container } = render(<EditConflictDialog tabId="other" />);
    expect(container).toBeEmptyDOMElement();
  });
});
