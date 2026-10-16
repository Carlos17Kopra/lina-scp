import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ReconnectBar } from "./ReconnectBar";

const onNow = vi.fn();
const onStop = vi.fn();
const bar = (retryAt: number) => <ReconnectBar state={{ attempt: 3, maxAttempts: 8, retryAt }} onNow={onNow} onStop={onStop} />;

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(100_000);
  onNow.mockClear();
  onStop.mockClear();
});
afterEach(() => vi.useRealTimers());

describe("ReconnectBar", () => {
  it("counts down to the next attempt and then shows that it connects", () => {
    const { container } = render(bar(104_000));
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
    render(bar(104_000));
    await user.click(screen.getByRole("button", { name: "Jetzt versuchen" }));
    expect(onNow).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(onStop).toHaveBeenCalled();
  });
});
