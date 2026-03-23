import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SignificanceBreakdown } from "./significance-breakdown";

describe("SignificanceBreakdown", () => {
  it("returns null when all counts are zero", () => {
    const { container } = render(<SignificanceBreakdown significant={0} notable={0} routine={0} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders legend with all category labels", () => {
    render(<SignificanceBreakdown significant={5} notable={3} routine={10} />);

    expect(screen.getByText("Significant")).toBeInTheDocument();
    expect(screen.getByText("Notable")).toBeInTheDocument();
    expect(screen.getByText("Routine")).toBeInTheDocument();
  });

  it("shows counts in legend", () => {
    const { container } = render(
      <SignificanceBreakdown significant={5} notable={3} routine={10} />,
    );

    const counts = container.querySelectorAll(".tabular-nums");
    const values = Array.from(counts).map((el) => el.textContent);
    expect(values).toContain("5");
    expect(values).toContain("3");
    expect(values).toContain("10");
  });

  it("omits zero-count segments from the bar", () => {
    const { container } = render(<SignificanceBreakdown significant={0} notable={5} routine={0} />);

    // Only one segment in the bar
    const barSegments = container.querySelectorAll(".h-full");
    expect(barSegments).toHaveLength(1);
  });

  it("renders all segments when all non-zero", () => {
    const { container } = render(<SignificanceBreakdown significant={1} notable={1} routine={1} />);

    const barSegments = container.querySelectorAll(".h-full");
    expect(barSegments).toHaveLength(3);
  });
});
