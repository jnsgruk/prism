import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  CancelHandlerRunResponseSchema,
  CancelRunResponseSchema,
  GetStatusResponseSchema,
  HandlersService,
  ListHandlersResponseSchema,
  ListRunsResponseSchema,
  SourceState,
  TriggerBackfillResponseSchema,
  TriggerHandlerResponseSchema,
  TriggerRunResponseSchema,
  TriggerTeamSyncResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { TestWrapper } from "@ps/test-utils";

const mockSources = [
  {
    name: "github-main",
    sourceType: "github",
    state: SourceState.IDLE,
    lastRun: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    itemsCollected: 42,
  },
];

const mockRuns = [
  {
    id: "run-1",
    sourceName: "github-main",
    startedAt: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    status: "completed",
    itemsCollected: 42,
    handlerName: "GithubIngestionHandler",
    handlerMethod: "run_ingestion",
  },
];

const mockHandlers = [
  {
    name: "GithubIngestionHandler",
    handlerType: "object",
    methods: ["run_ingestion", "backfill"],
  },
];

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(HandlersService, {
      getStatus: () => create(GetStatusResponseSchema, { sources: mockSources }),
      listRuns: () => create(ListRunsResponseSchema, { runs: mockRuns }),
      triggerRun: () => create(TriggerRunResponseSchema, {}),
      triggerBackfill: () => create(TriggerBackfillResponseSchema, {}),
      cancelRun: () => create(CancelRunResponseSchema, {}),
      listHandlers: () => create(ListHandlersResponseSchema, { handlers: mockHandlers }),
      triggerHandler: () => create(TriggerHandlerResponseSchema, {}),
      cancelHandlerRun: () => create(CancelHandlerRunResponseSchema, {}),
      triggerTeamSync: () => create(TriggerTeamSyncResponseSchema, {}),
    });
  }),
}));

describe("ingestion hooks", () => {
  describe("useIngestionStatus", () => {
    it("fetches and returns source statuses", async () => {
      const { useIngestionStatus } = await import("./use-ingestion");
      const { result } = renderHook(() => useIngestionStatus(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toHaveLength(1);
      expect(result.current.data?.[0]?.name).toBe("github-main");
      expect(result.current.data?.[0]?.state).toBe(SourceState.IDLE);
    });
  });

  describe("useListRuns", () => {
    it("fetches runs without source filter", async () => {
      const { useListRuns } = await import("./use-ingestion");
      const { result } = renderHook(() => useListRuns(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toHaveLength(1);
      expect(result.current.data?.[0]?.id).toBe("run-1");
    });

    it("accepts optional sourceName parameter", async () => {
      const { useListRuns } = await import("./use-ingestion");
      const { result } = renderHook(() => useListRuns("github-main"), {
        wrapper: TestWrapper,
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toHaveLength(1);
    });
  });

  describe("useTriggerRun", () => {
    it("triggers a run and succeeds", async () => {
      const { useTriggerRun } = await import("./use-ingestion");
      const { result } = renderHook(() => useTriggerRun(), { wrapper: TestWrapper });

      result.current.mutate("github-main");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useTriggerBackfill", () => {
    it("triggers a backfill with source and date", async () => {
      const { useTriggerBackfill } = await import("./use-ingestion");
      const { result } = renderHook(() => useTriggerBackfill(), { wrapper: TestWrapper });

      result.current.mutate({ sourceName: "github-main", sinceDate: "2026-01-01" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useCancelRun", () => {
    it("cancels a run and succeeds", async () => {
      const { useCancelRun } = await import("./use-ingestion");
      const { result } = renderHook(() => useCancelRun(), { wrapper: TestWrapper });

      result.current.mutate("github-main");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useListHandlers", () => {
    it("fetches available handlers", async () => {
      const { useListHandlers } = await import("./use-ingestion");
      const { result } = renderHook(() => useListHandlers(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toHaveLength(1);
      expect(result.current.data?.[0]?.name).toBe("GithubIngestionHandler");
    });
  });

  describe("useTriggerHandler", () => {
    it("triggers a handler and succeeds", async () => {
      const { useTriggerHandler } = await import("./use-ingestion");
      const { result } = renderHook(() => useTriggerHandler(), { wrapper: TestWrapper });

      result.current.mutate({
        handlerName: "MetricsComputeHandler",
        method: "compute_current_periods",
        key: "",
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useCancelHandlerRun", () => {
    it("cancels a handler run by ID", async () => {
      const { useCancelHandlerRun } = await import("./use-ingestion");
      const { result } = renderHook(() => useCancelHandlerRun(), { wrapper: TestWrapper });

      result.current.mutate("run-123");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useTriggerTeamSync", () => {
    it("triggers team sync and succeeds", async () => {
      const { useTriggerTeamSync } = await import("./use-ingestion");
      const { result } = renderHook(() => useTriggerTeamSync(), { wrapper: TestWrapper });

      result.current.mutate("github-main");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("handlersKeys", () => {
    it("builds hierarchical query keys", async () => {
      const { handlersKeys } = await import("./use-ingestion");
      expect(handlersKeys.all).toEqual(["handlers"]);
      expect(handlersKeys.status()).toEqual(["handlers", "status"]);
      expect(handlersKeys.runs()).toEqual(["handlers", "runs", undefined, undefined]);
      expect(handlersKeys.runs("github")).toEqual(["handlers", "runs", "github", undefined]);
      expect(handlersKeys.runs("github", "GithubIngestionHandler")).toEqual([
        "handlers",
        "runs",
        "github",
        "GithubIngestionHandler",
      ]);
      expect(handlersKeys.handlers()).toEqual(["handlers", "handlers"]);
    });
  });
});
