import { create } from "@bufbuild/protobuf";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { EnrichmentSchema } from "@ps/api/gen/prism/v1/reasoning_pb";

import { EnrichmentBadge, EnrichmentBadgeList } from "./enrichment-badge";

const makeEnrichment = (
  type: string,
  value: Record<string, unknown>,
): ReturnType<typeof create<typeof EnrichmentSchema>> =>
  create(EnrichmentSchema, {
    id: `e-${type}`,
    contributionId: "c-1",
    enrichmentType: type,
    valueJson: JSON.stringify(value),
    modelName: "test-model",
    createdAt: "2026-03-15T10:00:00Z",
  });

describe("EnrichmentBadge", () => {
  it("renders review_depth with score label", () => {
    const enrichment = makeEnrichment("review_depth", {
      score: 4,
      rationale: "Thorough review",
      confidence: 0.92,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/4\/5/)).toBeInTheDocument();
    expect(screen.getByText(/Depth/)).toBeInTheDocument();
  });

  it("renders sentiment with sentiment label", () => {
    const enrichment = makeEnrichment("sentiment", {
      sentiment: "constructive",
      rationale: "Helpful feedback",
      confidence: 0.85,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/constructive/)).toBeInTheDocument();
  });

  it("renders significance with significance label", () => {
    const enrichment = makeEnrichment("significance", {
      significance: "notable",
      rationale: "Important refactor",
      confidence: 0.78,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/notable/)).toBeInTheDocument();
  });

  it("renders topic with primary category", () => {
    const enrichment = makeEnrichment("topic", {
      primary_category: "security",
      rationale: "Auth changes",
      confidence: 0.9,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/security/)).toBeInTheDocument();
  });

  it("handles unknown enrichment type gracefully", () => {
    const enrichment = makeEnrichment("custom_type", { rationale: "test", confidence: 0.5 });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/custom_type/)).toBeInTheDocument();
  });

  it("handles empty valueJson without throwing", () => {
    const enrichment = create(EnrichmentSchema, {
      id: "e-1",
      contributionId: "c-1",
      enrichmentType: "review_depth",
      valueJson: "",
      modelName: "test-model",
      createdAt: "2026-03-15T10:00:00Z",
    });
    const { container } = render(<EnrichmentBadge enrichment={enrichment} />);
    expect(container.firstChild).not.toBeNull();
  });
});

describe("EnrichmentBadgeList", () => {
  it("renders empty fragment for empty array", () => {
    const { container } = render(<EnrichmentBadgeList enrichments={[]} />);
    expect(container.firstChild?.childNodes.length ?? 0).toBe(0);
  });

  it("renders multiple badges", () => {
    const enrichments = [
      makeEnrichment("review_depth", { score: 3, rationale: "ok", confidence: 0.7 }),
      makeEnrichment("sentiment", { sentiment: "neutral", rationale: "meh", confidence: 0.6 }),
    ];
    render(<EnrichmentBadgeList enrichments={enrichments} />);
    expect(screen.getByText(/3\/5/)).toBeInTheDocument();
    expect(screen.getByText(/neutral/)).toBeInTheDocument();
  });
});
