import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { createRouterTransport } from "@connectrpc/connect";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  GetStatusResponseSchema,
  HandlersService,
  ListRunsResponseSchema,
  SourceState,
  TriggerBackfillResponseSchema,
  TriggerRunResponseSchema,
} from "@ps/api/gen/prism/v1/handlers_pb";

import { TestWrapper } from "./test-wrapper";

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
  }),
}));

const renderPage = async (): Promise<void> => {
  const { default: IngestionPage } = await import("./ingestion-page");
  render(<IngestionPage />, { wrapper: TestWrapper });

  await waitFor(() => {
    expect(screen.getAllByText("github-main").length).toBeGreaterThanOrEqual(1);
  });
};

describe("IngestionPage", () => {
  afterEach(cleanup);

  it("renders source names and state badges", async () => {
    await renderPage();

    expect(screen.getAllByText("github-main").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("jira-project").length).toBeGreaterThanOrEqual(1);

    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(screen.getAllByText("Error").length).toBeGreaterThanOrEqual(1);
  });

  it("renders source type labels", async () => {
    await renderPage();

    expect(screen.getByText("github")).toBeInTheDocument();
    expect(screen.getByText("jira")).toBeInTheDocument();
  });

  it("renders Run Now and Backfill buttons for each source", async () => {
    await renderPage();

    expect(screen.getAllByRole("button", { name: /Run Now/i })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: /Backfill/i })).toHaveLength(2);
  });

  it("renders run history panel with table", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Run History")).toBeInTheDocument();
    });

    // Table should show run data — "Completed" appears as both filter button and status badge
    expect(screen.getAllByText("Completed").length).toBeGreaterThanOrEqual(2);
  });

  it("renders filter controls in run history panel", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Run History")).toBeInTheDocument();
    });

    // Source filter buttons
    expect(screen.getByRole("button", { name: "All sources" })).toBeInTheDocument();

    // Status filter buttons
    expect(screen.getByRole("button", { name: "Completed" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Failed" })).toBeInTheDocument();
  });
});
