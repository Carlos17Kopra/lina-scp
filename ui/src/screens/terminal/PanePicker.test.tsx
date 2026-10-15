import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import { makeHost } from "../../test/fixtures";
import { PanePicker } from "./PanePicker";

vi.mock("../../ipc/api");

const start = vi.fn(async () => undefined);
const openLocal = vi.fn(async () => undefined);
const closeSession = vi.fn(async () => undefined);
const picking: Tab = { id: "p1", title: "Neuer Bereich", kind: "ssh", status: "picking", origin: { kind: "ssh", hostId: "h1" } };

beforeEach(() => {
  start.mockClear();
  openLocal.mockClear();
  closeSession.mockClear();
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  useSessionsStore.setState({ ...useSessionsStore.getInitialState(), openLocal, closeSession }, true);
  useHostsStore.setState(
    {
      ...useHostsStore.getInitialState(),
      hosts: [
        makeHost({ id: "h1", name: "web-prod-01", address: "10.0.1.12" }),
        makeHost({ id: "h2", name: "db-prod-01", address: "10.0.1.20" }),
        makeHost({ id: "h3", name: "files", address: "10.0.1.30", protocolSsh: false }),
      ],
    },
    true,
  );
});

const options = () => screen.getAllByRole("option").map((option) => option.textContent);

describe("PanePicker", () => {
  it("preselects the same host and opens it with Enter", async () => {
    render(<PanePicker pane={picking} focused />);
    expect(options()[0]).toContain("Gleicher Host (web-prod-01)");
    expect(options().some((text) => text?.includes("files"))).toBe(false);
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
    await userEvent.type(screen.getByRole("combobox", { name: "Host suchen" }), "{Enter}");
    expect(start).toHaveBeenCalledWith("h1", "ssh", { paneId: "p1" });
  });

  it("filters by name and address and picks with the arrow keys", async () => {
    render(<PanePicker pane={picking} focused />);
    await userEvent.type(screen.getByRole("combobox", { name: "Host suchen" }), "10.0.1.2");
    expect(options()[0]).toContain("db-prod-01");
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(openLocal).toHaveBeenCalledWith({ paneId: "p1" });
  });

  it("offers the local terminal first for a pane split from a local one, and closes on Escape", async () => {
    render(<PanePicker pane={{ ...picking, kind: "local", origin: { kind: "local" } }} focused />);
    expect(options()[0]).toBe("Lokales Terminal");
    await userEvent.click(screen.getByRole("option", { name: /db-prod-01/ }));
    expect(start).toHaveBeenCalledWith("h2", "ssh", { paneId: "p1" });
    await userEvent.type(screen.getByRole("combobox", { name: "Host suchen" }), "{Escape}");
    expect(closeSession).toHaveBeenCalledWith("p1");
  });

  it("takes the keyboard whenever its pane becomes the focused one (review I2)", () => {
    const { rerender } = render(<PanePicker pane={picking} focused={false} />);
    const search = screen.getByRole("combobox", { name: "Host suchen" });
    expect(search).not.toHaveFocus();
    rerender(<PanePicker pane={picking} focused />);
    expect(search).toHaveFocus();
  });
});
