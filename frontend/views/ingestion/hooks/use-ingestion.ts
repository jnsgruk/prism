import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  CancelHandlerRunResponse,
  GetStatusResponse,
  HandlerRun,
  ListPipelineRunsResponse,
  PipelineRunSummary,
  SourceStatus,
  TriggerHandlerResponse,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { HandlersService } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { transport } from "@ps/api/transport";

type RefetchInterval = number | false | ((query: { state: { data: GetStatusResponse | undefined } }) => number | false);

const handlersClient = createClient(HandlersService, transport);

export const handlersKeys = {
  all: ["handlers"] as const,
  status: (): readonly ["handlers", "status"] => [...handlersKeys.all, "status"] as const,
  runs: (sourceName?: string, handlerName?: string) => [...handlersKeys.all, "runs", sourceName, handlerName] as const,
  handlers: (): readonly ["handlers", "handlers"] => [...handlersKeys.all, "handlers"] as const,
  pipelineRuns: () => [...handlersKeys.all, "pipelineRuns"] as const,
};

export const useIngestionStatus = (options?: { refetchInterval?: RefetchInterval }): UseQueryResult<SourceStatus[]> =>
  useQuery({
    queryKey: handlersKeys.status(),
    queryFn: () => handlersClient.getStatus({}),
    select: (data): SourceStatus[] => data.sources,
    refetchInterval: options?.refetchInterval,
  });

export const useListRuns = (
  sourceName?: string,
  options?: { refetchInterval?: number | false; handlerName?: string; ingestionOnly?: boolean },
): UseQueryResult<HandlerRun[]> =>
  useQuery({
    queryKey: handlersKeys.runs(sourceName, options?.handlerName),
    queryFn: () =>
      handlersClient.listRuns({
        sourceName,
        handlerName: options?.handlerName,
        ingestionOnly: options?.ingestionOnly ?? false,
      }),
    select: (data): HandlerRun[] => data.runs,
    refetchInterval: options?.refetchInterval,
  });

export const useListPipelineRuns = (options?: {
  refetchInterval?: number | false;
}): UseQueryResult<PipelineRunSummary[]> =>
  useQuery({
    queryKey: handlersKeys.pipelineRuns(),
    queryFn: () => handlersClient.listPipelineRuns({}),
    select: (data: ListPipelineRunsResponse): PipelineRunSummary[] => data.pipelines,
    refetchInterval: options?.refetchInterval,
  });

export const useTriggerHandler = (): UseMutationResult<
  TriggerHandlerResponse,
  Error,
  { handlerName: string; method: string; key: string; payload?: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { handlerName: string; method: string; key: string; payload?: string }) =>
      handlersClient.triggerHandler(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: handlersKeys.runs() });
      queryClient.invalidateQueries({ queryKey: handlersKeys.status() });
      queryClient.invalidateQueries({ queryKey: handlersKeys.handlers() });
    },
  });
};

export const useCancelHandlerRun = (): UseMutationResult<CancelHandlerRunResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (runId: string) => handlersClient.cancelHandlerRun({ runId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: handlersKeys.status() });
      queryClient.invalidateQueries({ queryKey: handlersKeys.runs() });
      queryClient.invalidateQueries({ queryKey: handlersKeys.handlers() });
    },
  });
};

export const useTriggerTeamSync = (): UseMutationResult<void, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (sourceName: string) => {
      await handlersClient.triggerTeamSync({ sourceName });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: handlersKeys.runs() });
    },
  });
};
