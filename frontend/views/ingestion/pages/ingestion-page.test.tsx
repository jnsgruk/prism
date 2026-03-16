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

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(HandlersService, {
      getStatus: () => create(GetStatusResponseSchema, { sources: mockSources }),
      listRuns: () => create(ListRunsResponseSchema, { runs: [] }),
      triggerRun: () => create(TriggerRunResponseSchema, {}),
      triggerBackfill: () => create(TriggerBackfillResponseSchema, {}),
    });
  }),
}));

const renderPage = async (): Promise<void> => {
  const { default: IngestionPage } = await import("./ingestion-page");
  render(<IngestionPage />, { wrapper: TestWrapper });

  await waitFor(() => {
    expect(screen.getByText("github-main")).toBeInTheDocument();
  });
};

describe("IngestionPage", () => {
  afterEach(cleanup);

  it("renders source names and state badges", async () => {
    await renderPage();

    expect(screen.getByText("github-main")).toBeInTheDocument();
    expect(screen.getByText("jira-project")).toBeInTheDocument();

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

  it("renders items collected for sources", async () => {
    await renderPage();

    // Items appear in both desktop and mobile stat sections
    expect(screen.getAllByText((142).toLocaleString()).length).toBeGreaterThanOrEqual(1);
  });
});
