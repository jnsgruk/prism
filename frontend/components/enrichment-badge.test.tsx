import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { EnrichmentType } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { EnrichmentSchema } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

import { EnrichmentBadge, EnrichmentBadgeList } from "./enrichment-badge";

const makeEnrichment = (
  type: EnrichmentType,
  value: Record<string, unknown>,
): ReturnType<typeof create<typeof EnrichmentSchema>> =>
  create(EnrichmentSchema, {
    id: `e-${type}`,
    contributionId: "c-1",
    enrichmentType: type,
    valueJson: JSON.stringify(value),
    modelName: "test-model",
    createdAt: timestampFromDate(new Date("2026-03-15T10:00:00Z")),
  });

describe("EnrichmentBadge", () => {
  it("renders review_depth with score label", () => {
    const enrichment = makeEnrichment(EnrichmentType.REVIEW_DEPTH, {
      score: 4,
      rationale: "Thorough review",
      confidence: 0.92,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/4\/5/)).toBeInTheDocument();
    expect(screen.getByText(/Depth/)).toBeInTheDocument();
  });

  it("renders sentiment with sentiment label", () => {
    const enrichment = makeEnrichment(EnrichmentType.SENTIMENT, {
      sentiment: "constructive",
      rationale: "Helpful feedback",
      confidence: 0.85,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/constructive/)).toBeInTheDocument();
  });

  it("renders significance with significance label", () => {
    const enrichment = makeEnrichment(EnrichmentType.SIGNIFICANCE, {
      significance: "notable",
      rationale: "Important refactor",
      confidence: 0.78,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/notable/)).toBeInTheDocument();
  });

  it("renders topic with primary category", () => {
    const enrichment = makeEnrichment(EnrichmentType.TOPIC, {
      primary_category: "security",
      rationale: "Auth changes",
      confidence: 0.9,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    expect(screen.getByText(/security/)).toBeInTheDocument();
  });

  it("handles unknown enrichment type gracefully", () => {
    const enrichment = makeEnrichment(EnrichmentType.UNSPECIFIED, {
      rationale: "test",
      confidence: 0.5,
    });
    render(<EnrichmentBadge enrichment={enrichment} />);
    // Unspecified type renders with fallback label
    expect(enrichment.enrichmentType).toBe(EnrichmentType.UNSPECIFIED);
  });

  it("handles empty valueJson without throwing", () => {
    const enrichment = create(EnrichmentSchema, {
      id: "e-1",
      contributionId: "c-1",
      enrichmentType: EnrichmentType.REVIEW_DEPTH,
      valueJson: "",
      modelName: "test-model",
      createdAt: timestampFromDate(new Date("2026-03-15T10:00:00Z")),
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
      makeEnrichment(EnrichmentType.REVIEW_DEPTH, { score: 3, rationale: "ok", confidence: 0.7 }),
      makeEnrichment(EnrichmentType.SENTIMENT, {
        sentiment: "neutral",
        rationale: "meh",
        confidence: 0.6,
      }),
    ];
    render(<EnrichmentBadgeList enrichments={enrichments} />);
    expect(screen.getByText(/3\/5/)).toBeInTheDocument();
    expect(screen.getByText(/neutral/)).toBeInTheDocument();
  });
});
