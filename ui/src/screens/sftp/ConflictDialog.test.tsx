import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { ConflictDialog } from "./ConflictDialog";

vi.mock("../../ipc/api");

beforeEach(() => {
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
  const s = useTransfersStore.getState();
  s.onEvent("t", { kind: "job", job: { id: 5, batchId: 2, direction: "upload", source: "/l/site.sql.gz", target: "/srv/site.sql.gz", rel: "site.sql.gz", kind: "file", size: 18_400_000, done: 0, state: "waitingDecision", note: null, error: null, retryable: true } });
  s.onEvent("t", { kind: "conflict", jobId: 5, source: { size: 18_400_000, mtime: 1_791_000_000 }, target: { size: 17_900_000, mtime: 1_790_000_000 } });
});

describe("ConflictDialog", () => {
  it("shows both sides and focuses Überspringen", () => {
    render(<ConflictDialog tabId="t" />);
    expect(screen.getByRole("dialog", { name: "Datei existiert bereits" })).toBeInTheDocument();
    expect(screen.getByText("site.sql.gz ist im Zielordner schon vorhanden.")).toBeInTheDocument();
    expect(screen.getByText("Neu · Lokal")).toBeInTheDocument();
    expect(screen.getByText("Vorhanden · Remote")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Überspringen" })).toHaveFocus();
  });

  it("answers with the chosen decision and the checkbox", async () => {
    render(<ConflictDialog tabId="t" />);
    await userEvent.click(screen.getByRole("checkbox", { name: "Für alle weiteren Konflikte übernehmen" }));
    await userEvent.click(screen.getByRole("button", { name: "Nur wenn neuer" }));
    expect(api.transferResolve).toHaveBeenCalledWith("s1", 5, "newer", true);
  });

  it("Escape cancels the whole batch", async () => {
    render(<ConflictDialog tabId="t" />);
    await userEvent.keyboard("{Escape}");
    expect(api.transferResolve).toHaveBeenCalledWith("s1", 5, "cancelBatch", false);
  });

  it("renders nothing without a conflict", () => {
    useTransfersStore.setState(useTransfersStore.getInitialState(), true);
    const { container } = render(<ConflictDialog tabId="t" />);
    expect(container).toBeEmptyDOMElement();
  });
});
