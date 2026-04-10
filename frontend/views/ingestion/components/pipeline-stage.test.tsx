import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { StageData } from "./pipeline-stage";
import { PipelineStage } from "./pipeline-stage";

describe("PipelineStage", () => {
  afterEach(cleanup);
  it("renders stage label for pending stage with no data", () => {
    render(<PipelineStage stageKey="metrics" stage={undefined} isCurrentStage={false} />);
    expect(screen.getByText("Metrics")).toBeDefined();
  });

  it("renders handler rows for multi-handler stages", () => {
    const stage: StageData = {
      status: "running",
      handlers: [
        { name: "Github", status: "completed", items: 42 },
        { name: "Jira", status: "running" },
        { name: "Discourse", status: "pending" },
      ],
    };
    render(<PipelineStage stageKey="ingestion" stage={stage} isCurrentStage={true} />);
    expect(screen.getByText("Ingestion")).toBeDefined();
    expect(screen.getByText("Github")).toBeDefined();
    expect(screen.getByText("42")).toBeDefined();
    expect(screen.getByText("Jira")).toBeDefined();
    expect(screen.getByText("Discourse")).toBeDefined();
  });

  it("renders completed single-handler stage with handler detail", () => {
    const stage: StageData = {
      status: "completed",
      handlers: [{ name: "Compute", status: "completed" }],
    };
    render(<PipelineStage stageKey="metrics" stage={stage} isCurrentStage={false} />);
    expect(screen.getByText("Metrics")).toBeDefined();
    expect(screen.getByText("Compute")).toBeDefined();
  });

  it("renders failed handler with error text", () => {
    const stage: StageData = {
      status: "failed",
      handlers: [{ name: "Jira", status: "failed", error: "auth token expired" }],
    };
    render(<PipelineStage stageKey="ingestion" stage={stage} isCurrentStage={false} />);
    expect(screen.getByText("auth token expired")).toBeDefined();
  });

  it("renders skipped stage with reduced opacity", () => {
    const stage: StageData = { status: "skipped", handlers: [] };
    const { container } = render(<PipelineStage stageKey="embedding" stage={stage} isCurrentStage={false} />);
    const stageDiv = container.firstElementChild;
    expect(stageDiv?.className).toContain("opacity-50");
  });
});
