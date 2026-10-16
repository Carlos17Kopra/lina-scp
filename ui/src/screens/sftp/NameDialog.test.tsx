import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { NameDialog } from "./NameDialog";

describe("NameDialog", () => {
  it("submits once on a double Enter", async () => {
    const onSubmit = vi.fn(() => new Promise<void>(() => {}));
    render(<NameDialog mode="new" onSubmit={onSubmit} onClose={() => {}} />);
    await userEvent.type(screen.getByRole("textbox"), "neu{Enter}{Enter}");
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });
});
