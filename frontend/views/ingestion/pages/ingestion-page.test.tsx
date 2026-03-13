import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { createRouterTransport } from "@connectrpc/connect";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  GetStatusResponseSchema,
  IngestionService,
  ListRunsResponseSchema,
  SourceState,
  TriggerBackfillResponseSchema,
  TriggerRunResponseSchema,
} from "@ps/api/gen/prism/v1/ingestion_pb";

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
  },
];

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(IngestionService, {
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
    expect(screen.getByText("Run History")).toBeInTheDocument();
  });
};

describe("IngestionPage", () => {
  afterEach(cleanup);

  it("renders source status cards with names and state badges", async () => {
    await renderPage();

    // Card titles use data-slot="card-title"
    const cardTitles = screen.getAllByText(
      (_content, element) => element?.getAttribute("data-slot") === "card-title",
    );
    const titleTexts = cardTitles.map((el) => el.textContent);
    expect(titleTexts).toContain("github-main");
    expect(titleTexts).toContain("jira-project");

    // State badges: Idle and Error (may appear multiple times due to table)
    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(screen.getAllByText("Error").length).toBeGreaterThanOrEqual(1);
  });

  it("renders source type labels on cards", async () => {
    await renderPage();

    expect(screen.getByText("github")).toBeInTheDocument();
    expect(screen.getByText("jira")).toBeInTheDocument();
  });

  it("renders run history table", async () => {
    await renderPage();

    const table = screen.getByRole("table");
    const tableScope = within(table);

    // Status badges inside the table
    expect(tableScope.getByText("Completed")).toBeInTheDocument();
    expect(tableScope.getByText("Failed")).toBeInTheDocument();

    // Error message is no longer in the table — it's shown in the detail dialog on row click
  });

  it("renders Run Now and Backfill buttons for each source", async () => {
    await renderPage();

    expect(screen.getAllByRole("button", { name: /Run Now/i })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: /Backfill/i })).toHaveLength(2);
  });

  it("renders source filter buttons in run history section", async () => {
    await renderPage();

    expect(screen.getByRole("button", { name: "All" })).toBeInTheDocument();
  });
});
