import { create } from "@bufbuild/protobuf";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { Code, ConnectError, createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";

import {
  CancelPipelineResponseSchema,
  GetPipelineStatusResponseSchema,
  HandlersService,
  PipelineInfoSchema,
  TriggerPipelineResponseSchema,
  type TriggerPipelineRequest,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { TestWrapper } from "@ps/test-utils";

const mockPipeline = create(PipelineInfoSchema, {
  id: "pipe-1",
  status: "running",
  currentStage: "ingestion",
  startedAt: timestampFromDate(new Date("2026-03-31T10:00:00Z")),
  stagesJson: JSON.stringify({
    ingestion: {
      status: "running",
      handlers: [
        { name: "Github", status: "running" },
        { name: "Jira", status: "pending" },
      ],
    },
  }),
});

const calls = vi.hoisted(() => ({ trigger: vi.fn<(request: TriggerPipelineRequest) => void>(), reject: false }));

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(HandlersService, {
      getPipelineStatus: () =>
        create(GetPipelineStatusResponseSchema, {
          current: mockPipeline,
          recent: [],
        }),
      triggerPipeline: (request) => {
        calls.trigger(request);
        if (calls.reject) throw new ConnectError("Delivery uncertain", Code.Unavailable);
        return create(TriggerPipelineResponseSchema, { pipelineId: "pipe-2" });
      },
      cancelPipeline: () => create(CancelPipelineResponseSchema, {}),
    });
  }),
}));

describe("pipeline hooks", () => {
  describe("usePipelineStatus", () => {
    it("fetches pipeline status with current pipeline", async () => {
      const { usePipelineStatus } = await import("./use-pipeline");
      const { result } = renderHook(() => usePipelineStatus(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.current?.id).toBe("pipe-1");
      expect(result.current.data?.current?.status).toBe("running");
      expect(result.current.data?.current?.currentStage).toBe("ingestion");
    });
  });

  describe("useCurrentPipeline", () => {
    it("extracts current pipeline and recent list", async () => {
      const { useCurrentPipeline } = await import("./use-pipeline");
      const { result } = renderHook(() => useCurrentPipeline(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isLoading).toBe(false));
      expect(result.current.current?.id).toBe("pipe-1");
      expect(result.current.recent).toHaveLength(0);
    });
  });

  describe("useTriggerPipeline", () => {
    it("triggers a pipeline and returns pipeline ID", async () => {
      const { useTriggerPipeline } = await import("./use-pipeline");
      const { result } = renderHook(() => useTriggerPipeline(), { wrapper: TestWrapper });

      result.current.mutate(undefined);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.pipelineId).toBe("pipe-2");
    });

    it("reuses the All submission ID after an uncertain response", async () => {
      calls.trigger.mockClear();
      calls.reject = true;
      const { useTriggerPipeline } = await import("./use-pipeline");
      const { result } = renderHook(() => useTriggerPipeline(), { wrapper: TestWrapper });
      result.current.mutate({ sinceDate: "2024-01-01" });
      await waitFor(() => expect(result.current.isError).toBe(true));
      calls.reject = false;
      result.current.mutate({ sinceDate: "2024-01-01" });
      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(calls.trigger.mock.calls[0]![0].submissionId).toBe(calls.trigger.mock.calls[1]![0].submissionId);
      expect(calls.trigger.mock.calls[1]![0].scope).toBeUndefined();
    });
  });

  describe("useCancelPipeline", () => {
    it("cancels a pipeline and succeeds", async () => {
      const { useCancelPipeline } = await import("./use-pipeline");
      const { result } = renderHook(() => useCancelPipeline(), { wrapper: TestWrapper });

      result.current.mutate("pipe-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("pipelineKeys", () => {
    it("builds hierarchical query keys", async () => {
      const { pipelineKeys } = await import("./use-pipeline");
      expect(pipelineKeys.all).toEqual(["pipeline"]);
      expect(pipelineKeys.status()).toEqual(["pipeline", "status"]);
    });
  });
});
