import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { KeepDialog } from "./KeepDialog";

vi.mock("../../ipc/api");

const props = { sessionId: "s1", hostId: "h1", localDir: "/l", remoteDir: "/srv" };

beforeEach(() => {
  localStorage.clear();
  vi.mocked(api.keepStart).mockResolvedValue();
});

describe("KeepDialog", () => {
  it("starts with both folders and the remembered patterns", async () => {
    localStorage.setItem("linascp.sync.exclude.h1", ".git");
    const close = vi.fn();
    render(<KeepDialog {...props} onClose={close} />);
    expect(screen.getByText("/l → /srv")).toBeInTheDocument();
    expect(screen.getByText(/Gelöschtes bleibt auf dem Server/)).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Ausschließen" }), ", *.tmp");
    await userEvent.click(screen.getByRole("button", { name: "Starten" }));
    expect(api.keepStart).toHaveBeenCalledWith("s1", "/l", "/srv", [".git", "*.tmp"]);
    await vi.waitFor(() => expect(close).toHaveBeenCalled());
    expect(localStorage.getItem("linascp.sync.exclude.h1")).toBe(".git, *.tmp");
  });

  it("stays open with the error when starting fails", async () => {
    vi.mocked(api.keepStart).mockRejectedValue({ type: "fs", kind: "notFound", path: "/srv", message: "not found" });
    const close = vi.fn();
    render(<KeepDialog {...props} onClose={close} />);
    await userEvent.click(screen.getByRole("button", { name: "Starten" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Starten fehlgeschlagen: not found");
    expect(close).not.toHaveBeenCalled();
  });
});
