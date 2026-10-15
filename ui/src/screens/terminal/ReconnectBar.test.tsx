import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Tab } from "../../stores/sessions";
import { ReconnectBar } from "./ReconnectBar";

vi.mock("../../ipc/api");

const reason = { type: "network", kind: "other", message: "connection lost" } as const;
const tab = (retryAt: number): Tab => ({
  id: "p",
  title: "web",
  kind: "ssh",
  sessionId: "s-1",
  status: "reconnecting",
  reconnect: { attempt: 3, maxAttempts: 8, retryAt, reason },
});

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(100_000);
  vi.mocked(api.sessionReconnectNow).mockResolvedValue(undefined);
  vi.mocked(api.sessionReconnectStop).mockResolvedValue(undefined);
});
afterEach(() => vi.useRealTimers());

describe("ReconnectBar", () => {
  it("counts down to the next attempt and then shows that it connects", () => {
    const { container } = render(<ReconnectBar tab={tab(104_000)} />);
    expect(container).toHaveTextContent("Verbindung unterbrochen · Versuch 3 von 8 in 4 s");
    act(() => vi.advanceTimersByTime(1000));
    expect(container).toHaveTextContent("in 3 s");
    act(() => vi.advanceTimersByTime(3000));
    expect(container).toHaveTextContent("Versuch 3 von 8 · Verbinde …");
    // Screen readers hear the attempt, not every tick of the countdown.
    expect(screen.getByRole("status")).toHaveTextContent(/^Verbindung unterbrochen · Versuch 3 von 8$/);
  });

  it("retries now and stops", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<ReconnectBar tab={tab(104_000)} />);
    await user.click(screen.getByRole("button", { name: "Jetzt versuchen" }));
    expect(api.sessionReconnectNow).toHaveBeenCalledWith("s-1");
    await user.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(api.sessionReconnectStop).toHaveBeenCalledWith("s-1");
  });
});
