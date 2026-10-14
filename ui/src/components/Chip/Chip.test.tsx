import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Card } from "../Card/Card";
import { Chip } from "./Chip";

describe("Chip and Card", () => {
  it("renders chips with their tone", () => {
    render(
      <Chip tone="amber" mono>
        SFTP
      </Chip>,
    );
    expect(screen.getByText("SFTP")).toHaveAttribute("data-tone", "amber");
  });

  it("defaults chips to neutral and marks selected cards", () => {
    render(
      <>
        <Chip>prod</Chip>
        <Card selected data-testid="card">
          web-prod-01
        </Card>
      </>,
    );
    expect(screen.getByText("prod")).toHaveAttribute("data-tone", "neutral");
    expect(screen.getByTestId("card")).toHaveAttribute("data-selected", "true");
  });
});
