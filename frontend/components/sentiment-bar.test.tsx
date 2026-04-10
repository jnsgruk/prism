import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SentimentBar } from "./sentiment-bar";

describe("SentimentBar", () => {
  it("returns null when all counts are zero", () => {
    const { container } = render(<SentimentBar constructive={0} neutral={0} critical={0} hostile={0} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders legend labels for non-zero segments", () => {
    render(<SentimentBar constructive={5} neutral={3} critical={2} hostile={0} />);

    expect(screen.getByText(/Constructive/)).toBeInTheDocument();
    expect(screen.getByText(/Neutral/)).toBeInTheDocument();
    expect(screen.getByText(/Critical/)).toBeInTheDocument();
    // Hostile is 0, so it should not appear in legend
    expect(screen.queryByText(/Hostile/)).toBeNull();
  });

  it("shows counts in legend", () => {
    render(<SentimentBar constructive={10} neutral={0} critical={0} hostile={0} />);
    expect(screen.getByText(/Constructive 10/)).toBeInTheDocument();
  });

  it("renders all four segments when all non-zero", () => {
    render(<SentimentBar constructive={1} neutral={1} critical={1} hostile={1} />);

    // Each segment appears in both tooltip and legend, so use getAllByText
    expect(screen.getAllByText(/Constructive/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/Neutral/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/Critical/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/Hostile/).length).toBeGreaterThanOrEqual(1);
  });

  it("omits zero-count segments from the bar and legend", () => {
    const { container } = render(<SentimentBar constructive={5} neutral={0} critical={0} hostile={0} />);

    // Only one segment should be rendered in the bar
    const barSegments = container.querySelectorAll(".h-full");
    expect(barSegments).toHaveLength(1);
  });
});
