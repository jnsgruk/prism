import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Platform, RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import {
  CancelHandlerRunResponseSchema,
  CancelPipelineResponseSchema,
  GetPipelineStatusResponseSchema,
  GetStatusResponseSchema,
  HandlersService,
  HandlerRunSchema,
  ListHandlersResponseSchema,
  ListRunsResponseSchema,
  SourceState,
  SourceStatusSchema,
  TriggerHandlerResponseSchema,
  TriggerPipelineResponseSchema,
  TriggerTeamSyncResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { TestWrapper } from "@ps/test-utils";

const mockSources = [
  create(SourceStatusSchema, {
    name: "github-main",
    sourceType: Platform.GITHUB,
    state: SourceState.IDLE,
    lastRun: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    itemsCollected: 42,
  }),
];

const mockRuns = [
  create(HandlerRunSchema, {
    id: "run-1",
    sourceName: "github-main",
    startedAt: timestampFromDate(new Date("2026-03-12T10:00:00Z")),
    status: RunStatus.COMPLETED,
    itemsCollected: 42,
    handlerName: "GithubIngestionHandler",
    handlerMethod: "run_ingestion",
  }),
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
      listHandlers: () => create(ListHandlersResponseSchema, { handlers: mockHandlers }),
      triggerHandler: () => create(TriggerHandlerResponseSchema, {}),
      cancelHandlerRun: () => create(CancelHandlerRunResponseSchema, {}),
      triggerTeamSync: () => create(TriggerTeamSyncResponseSchema, {}),
      getPipelineStatus: () => create(GetPipelineStatusResponseSchema, {}),
      triggerPipeline: () => create(TriggerPipelineResponseSchema, { pipelineId: "pipe-1" }),
      cancelPipeline: () => create(CancelPipelineResponseSchema, {}),
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
