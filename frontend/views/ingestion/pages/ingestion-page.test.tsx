import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { createRouterTransport } from "@connectrpc/connect";
import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  GetStatusResponseSchema,
  HandlersService,
  ListRunsResponseSchema,
  SourceState,
  TriggerBackfillResponseSchema,
  TriggerRunResponseSchema,
} from "@ps/api/gen/prism/v1/handlers_pb";
import {
  GetEnrichmentPipelineStatusResponseSchema,
  ReasoningService,
} from "@ps/api/gen/prism/v1/reasoning_pb";
import { renderWithProviders, setupCleanup } from "@ps/test-utils";

const mockSources = [
  {
    name: "github-main",
    sourceType: "github",
    state: SourceState.IDLE,
    lastRun: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    itemsCollected: 142,
    rateLimitInfo: {},
  },
  {
    name: "jira-project",
    sourceType: "jira",
    state: SourceState.ERROR,
    lastRun: timestampFromDate(new Date("2026-03-11T08:30:00Z")),
    itemsCollected: 0,
    rateLimitInfo: {},
  },
];

const mockRuns = [
  {
    id: "run-1",
    sourceName: "github-main",
    startedAt: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    completedAt: timestampFromDate(new Date("2026-03-12T10:05:00Z")),
    status: "completed",
    itemsCollected: 142,
    rateLimitWaitsSeconds: 0,
    handlerName: "GithubIngestionHandler",
    handlerMethod: "run_ingestion",
  },
  {
    id: "run-2",
    sourceName: "jira-project",
    startedAt: timestampFromDate(new Date("2026-03-11T08:30:00Z")),
    completedAt: timestampFromDate(new Date("2026-03-11T08:31:00Z")),
    status: "failed",
    itemsCollected: 0,
    errorMessage: "Authentication failed: invalid token",
    rateLimitWaitsSeconds: 0,
    handlerName: "GithubIngestionHandler",
    handlerMethod: "run_ingestion",
  },
];

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(HandlersService, {
      getStatus: () => create(GetStatusResponseSchema, { sources: mockSources }),
      listRuns: () => create(ListRunsResponseSchema, { runs: mockRuns }),
      triggerRun: () => create(TriggerRunResponseSchema, {}),
      triggerBackfill: () => create(TriggerBackfillResponseSchema, {}),
    });
    service(ReasoningService, {
      getEnrichmentPipelineStatus: () =>
        create(GetEnrichmentPipelineStatusResponseSchema, {
          pendingCount: 0n,
          totalEnrichments: 100n,
          byType: [],
        }),
    });
  }),
}));

const renderPage = async (): Promise<void> => {
  const { default: IngestionPage } = await import("./ingestion-page");
  renderWithProviders(<IngestionPage />);

  await waitFor(() => {
    expect(screen.getAllByText("github-main").length).toBeGreaterThanOrEqual(1);
  });
};

describe("IngestionPage", () => {
  setupCleanup();

  it("renders source names and state labels", async () => {
    await renderPage();

    expect(screen.getAllByText("github-main").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("jira-project").length).toBeGreaterThanOrEqual(1);

    expect(screen.getAllByText("Idle").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("Error").length).toBeGreaterThanOrEqual(1);
  });

  it("renders summary strip", async () => {
    await renderPage();

    // Summary should show idle count and Run All button
    expect(screen.getByRole("button", { name: /Run All/i })).toBeInTheDocument();
  });

  it("renders Run and Backfill controls for each source", async () => {
    await renderPage();

    // Each idle source gets a Run button (text is hidden on mobile but button exists)
    const runButtons = screen.getAllByRole("button", { name: /Run/i });
    // At least 2 for sources + 1 for Run All + possibly 1 for enrichment
    expect(runButtons.length).toBeGreaterThanOrEqual(3);
  });

  it("renders run history panel with status pills", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Run History")).toBeInTheDocument();
    });

    // Collapsed header shows status counts (number and label are split across elements)
    expect(screen.getByText((_, el) => el?.textContent === "1 completed")).toBeInTheDocument();
    expect(screen.getByText((_, el) => el?.textContent === "1 failed")).toBeInTheDocument();
  });

  it("expands run history to show filters and table", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Run History")).toBeInTheDocument();
    });

    // Expand the collapsible run history
    screen.getByText("Run History").click();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Completed" })).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: "Failed" })).toBeInTheDocument();
  });
});
