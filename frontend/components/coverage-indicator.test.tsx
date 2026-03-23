import { create } from "@bufbuild/protobuf";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TypeCoverageSchema } from "@ps/api/gen/canonical/prism/v1/insights_pb";

import { CoverageIndicator } from "./coverage-indicator";

const makeCoverage = (
  type: string,
  enriched: number,
  eligible: number,
): ReturnType<typeof create<typeof TypeCoverageSchema>> =>
  create(TypeCoverageSchema, { enrichmentType: type, enriched, eligible });

describe("CoverageIndicator", () => {
  it("returns null for empty input", () => {
    const { container } = render(<CoverageIndicator byType={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders type labels correctly", () => {
    render(
      <CoverageIndicator
        byType={[
          makeCoverage("review_depth", 10, 20),
          makeCoverage("sentiment", 5, 10),
          makeCoverage("significance", 3, 15),
          makeCoverage("topic", 0, 5),
        ]}
      />,
    );

    expect(screen.getByText("Review depth")).toBeInTheDocument();
    expect(screen.getByText("Sentiment")).toBeInTheDocument();
    expect(screen.getByText("PR significance")).toBeInTheDocument();
    expect(screen.getByText("Topic classification")).toBeInTheDocument();
  });

  it("shows enriched/eligible ratio", () => {
    render(<CoverageIndicator byType={[makeCoverage("review_depth", 10, 20)]} />);
    expect(screen.getAllByText("10/20").length).toBeGreaterThanOrEqual(1);
  });

  it("handles zero eligible without division error", () => {
    render(<CoverageIndicator byType={[makeCoverage("review_depth", 0, 0)]} />);
    expect(screen.getByText("0/0")).toBeInTheDocument();
  });

  it("passes through unknown type labels as-is", () => {
    render(<CoverageIndicator byType={[makeCoverage("custom_type", 1, 5)]} />);
    expect(screen.getByText("custom_type")).toBeInTheDocument();
  });
});
