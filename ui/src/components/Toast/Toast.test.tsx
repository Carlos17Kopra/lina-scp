import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type ToastOptions, ToastProvider, useToast } from "./Toast";

function Trigger({ options }: { options: ToastOptions }) {
  const toast = useToast();
  return (
    <button type="button" onClick={() => toast.show(options)}>
      Zeigen
    </button>
  );
}

function renderWith(options: ToastOptions) {
  return render(
    <ToastProvider>
      <Trigger options={options} />
    </ToastProvider>,
  );
}

describe("Toast", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows a status message and dismisses it", async () => {
    const user = userEvent.setup();
    renderWith({ tone: "green", message: "Gespeichert" });
    await user.click(screen.getByRole("button", { name: "Zeigen" }));
    expect(screen.getByRole("status")).toHaveTextContent("Gespeichert");
    await user.click(screen.getByRole("button", { name: "Meldung schließen" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("dismisses itself after the duration", () => {
    vi.useFakeTimers();
    renderWith({ tone: "accent", message: "Kopiert", durationMs: 1000 });
    fireEvent.click(screen.getByRole("button", { name: "Zeigen" }));
    expect(screen.getByRole("status")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows errors as alerts with copyable details", async () => {
    const user = userEvent.setup();
    renderWith({ tone: "red", message: "Speichern fehlgeschlagen", details: '{"type":"io"}' });
    await user.click(screen.getByRole("button", { name: "Zeigen" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    await user.click(screen.getByRole("button", { name: "Details kopieren" }));
    await expect(navigator.clipboard.readText()).resolves.toBe('{"type":"io"}');
    expect(await screen.findByRole("button", { name: "Kopiert" })).toBeInTheDocument();
  });

  it("requires the provider", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() => render(<Trigger options={{ tone: "accent", message: "x" }} />)).toThrow(/ToastProvider/);
  });
});
