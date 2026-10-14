import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Tabs, tabPanelId } from "./Tabs";

const tabs = [
  { id: "general", label: "Allgemein" },
  { id: "auth", label: "Authentifizierung" },
  { id: "connection", label: "Verbindung" },
  { id: "sftp", label: "SFTP" },
];

function Harness({ orientation }: { orientation?: "horizontal" | "vertical" }) {
  const [value, setValue] = useState("general");
  return (
    <>
      <Tabs label="Host-Einstellungen" tabs={tabs} value={value} onChange={setValue} orientation={orientation} idBase="ed" />
      <div role="tabpanel" id={tabPanelId("ed", value)}>
        {value}
      </div>
    </>
  );
}

describe("Tabs", () => {
  it("renders a tablist with the selected tab wired to its panel", () => {
    render(<Harness />);
    expect(screen.getByRole("tablist", { name: "Host-Einstellungen" })).toHaveAttribute("aria-orientation", "horizontal");
    const general = screen.getByRole("tab", { name: "Allgemein" });
    expect(general).toHaveAttribute("aria-selected", "true");
    expect(general).toHaveAttribute("aria-controls", "ed-panel-general");
    expect(general).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("tab", { name: "SFTP" })).toHaveAttribute("tabindex", "-1");
  });

  it("supports ArrowRight/ArrowLeft with wrap-around and Home/End", async () => {
    render(<Harness />);
    screen.getByRole("tab", { name: "Allgemein" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "SFTP" })).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("sftp");
  });

  it("uses ArrowDown/ArrowUp when vertical", async () => {
    render(<Harness orientation="vertical" />);
    screen.getByRole("tab", { name: "Allgemein" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveFocus();
  });
});
