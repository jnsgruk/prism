import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { createRouterTransport } from "@connectrpc/connect";
import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  GetPipelineStatusResponseSchema,
  GetStatusResponseSchema,
  HandlersService,
  HandlerRunSchema,
  ListRunsResponseSchema,
  SourceState,
  SourceStatusSchema,
  TriggerBackfillResponseSchema,
  TriggerRunResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { Platform, RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import {
  ConfigService,
  ListSourcesResponseSchema,
  SourceConfigSchema,
} from "@ps/api/gen/canonical/prism/v1/config_pb";
import {
  GetEmbeddingStatusResponseSchema,
  GetEnrichmentPipelineStatusResponseSchema,
  ReasoningService,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { renderWithProviders, setupCleanup } from "@ps/test-utils";

const mockSources = [
  create(SourceStatusSchema, {
    name: "github-main",
    sourceType: Platform.GITHUB,
    state: SourceState.IDLE,
    lastRun: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    itemsCollected: 142,
  }),
  create(SourceStatusSchema, {
    name: "jira-project",
    sourceType: Platform.JIRA,
    state: SourceState.ERROR,
    lastRun: timestampFromDate(new Date("2026-03-11T08:30:00Z")),
    itemsCollected: 0,
  }),
];

const mockSourceConfigs = [
  create(SourceConfigSchema, {
    id: "src-1",
    name: "github-main",
    sourceType: Platform.GITHUB,
    enabled: true,
  }),
  create(SourceConfigSchema, {
    id: "src-2",
    name: "jira-project",
    sourceType: Platform.JIRA,
    enabled: true,
  }),
];

const mockRuns = [
  create(HandlerRunSchema, {
    id: "run-1",
    sourceName: "github-main",
    startedAt: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    completedAt: timestampFromDate(new Date("2026-03-12T10:05:00Z")),
    status: RunStatus.COMPLETED,
    itemsCollected: 142,
    rateLimitWaitsSeconds: 0,
    handlerName: "GithubIngestionHandler",
    handlerMethod: "run_ingestion",
  }),
  create(HandlerRunSchema, {
    id: "run-2",
    sourceName: "jira-project",
    startedAt: timestampFromDate(new Date("2026-03-11T08:30:00Z")),
    completedAt: timestampFromDate(new Date("2026-03-11T08:31:00Z")),
    status: RunStatus.FAILED,
    itemsCollected: 0,
    errorMessage: "Authentication failed: invalid token",
    rateLimitWaitsSeconds: 0,
    handlerName: "GithubIngestionHandler",
    handlerMethod: "run_ingestion",
  }),
];

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(HandlersService, {
      getStatus: () => create(GetStatusResponseSchema, { sources: mockSources }),
      listRuns: () => create(ListRunsResponseSchema, { runs: mockRuns }),
      triggerRun: () => create(TriggerRunResponseSchema, {}),
      triggerBackfill: () => create(TriggerBackfillResponseSchema, {}),
      getPipelineStatus: () => create(GetPipelineStatusResponseSchema, {}),
    });
    service(ConfigService, {
      listSources: () => create(ListSourcesResponseSchema, { sources: mockSourceConfigs }),
    });
    service(ReasoningService, {
      getEnrichmentPipelineStatus: () =>
        create(GetEnrichmentPipelineStatusResponseSchema, {
          pendingCount: 0n,
          totalEnrichments: 100n,
          byType: [],
        }),
      getEmbeddingStatus: () =>
        create(GetEmbeddingStatusResponseSchema, {
          embeddedCount: 120n,
          queuedCount: 380n,
          coveragePercent: 24.0,
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

  it("renders pipeline controls in card header", async () => {
    await renderPage();

    // Pipeline Run button is the primary control (no more "Run All")
    expect(screen.getByRole("button", { name: /Run Pipeline/i })).toBeInTheDocument();
  });

  it("renders AI handler rows in the source list", async () => {
    await renderPage();

    // Enrichments and Embeddings are now rows in the same card
    expect(screen.getByText("Enrichments")).toBeInTheDocument();
    expect(screen.getByText("Embeddings")).toBeInTheDocument();
  });

  it("renders run history panel with status pills", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Run History")).toBeInTheDocument();
    });

    // Collapsed header shows status counts
    expect(screen.getByText((_, el) => el?.textContent === "1 completed")).toBeInTheDocument();
    expect(screen.getByText((_, el) => el?.textContent === "1 failed")).toBeInTheDocument();
  });

  it("expands run history to show filters and table", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Run History")).toBeInTheDocument();
    });

    screen.getByText("Run History").click();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Completed" })).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: "Failed" })).toBeInTheDocument();
  });
});
