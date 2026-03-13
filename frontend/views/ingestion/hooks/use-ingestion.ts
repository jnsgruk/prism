import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  CancelRunResponse,
  GetStatusResponse,
  HandlerInfo,
  IngestionRun,
  SourceStatus,
  TriggerBackfillResponse,
  TriggerHandlerResponse,
  TriggerRunResponse,
} from "@ps/api/gen/prism/v1/ingestion_pb";
import { IngestionService } from "@ps/api/gen/prism/v1/ingestion_pb";
import { transport } from "@ps/api/transport";

type RefetchInterval =
  | number
  | false
  | ((query: { state: { data: GetStatusResponse | undefined } }) => number | false);

const ingestionClient = createClient(IngestionService, transport);

export const ingestionKeys = {
  all: ["ingestion"] as const,
  status: (): readonly ["ingestion", "status"] => [...ingestionKeys.all, "status"] as const,
  runs: (sourceName?: string, handlerName?: string) =>
    [...ingestionKeys.all, "runs", sourceName, handlerName] as const,
  handlers: (): readonly ["ingestion", "handlers"] => [...ingestionKeys.all, "handlers"] as const,
};

export const useIngestionStatus = (options?: {
  refetchInterval?: RefetchInterval;
}): UseQueryResult<SourceStatus[], Error> =>
  useQuery({
    queryKey: ingestionKeys.status(),
    queryFn: () => ingestionClient.getStatus({}),
    select: (data): SourceStatus[] => data.sources,
    refetchInterval: options?.refetchInterval,
  });

export const useListRuns = (
  sourceName?: string,
  options?: { refetchInterval?: number | false; handlerName?: string },
): UseQueryResult<IngestionRun[], Error> =>
  useQuery({
    queryKey: ingestionKeys.runs(sourceName, options?.handlerName),
    queryFn: () => ingestionClient.listRuns({ sourceName, handlerName: options?.handlerName }),
    select: (data): IngestionRun[] => data.runs,
    refetchInterval: options?.refetchInterval,
  });

export const useTriggerRun = (): UseMutationResult<TriggerRunResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (sourceName: string) => ingestionClient.triggerRun({ sourceName }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ingestionKeys.status() });
      queryClient.invalidateQueries({ queryKey: ingestionKeys.runs() });
    },
  });
};

export const useTriggerBackfill = (): UseMutationResult<
  TriggerBackfillResponse,
  Error,
  { sourceName: string; sinceDate: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceName: string; sinceDate: string }) =>
      ingestionClient.triggerBackfill(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ingestionKeys.status() });
      queryClient.invalidateQueries({ queryKey: ingestionKeys.runs() });
    },
  });
};

export const useCancelRun = (): UseMutationResult<CancelRunResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (sourceName: string) => ingestionClient.cancelRun({ sourceName }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ingestionKeys.status() });
      queryClient.invalidateQueries({ queryKey: ingestionKeys.runs() });
    },
  });
};

export const useListHandlers = (): UseQueryResult<HandlerInfo[], Error> =>
  useQuery({
    queryKey: ingestionKeys.handlers(),
    queryFn: () => ingestionClient.listHandlers({}),
    select: (data): HandlerInfo[] => data.handlers,
  });

export const useTriggerHandler = (): UseMutationResult<
  TriggerHandlerResponse,
  Error,
  { handlerName: string; method: string; key: string; payload?: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { handlerName: string; method: string; key: string; payload?: string }) =>
      ingestionClient.triggerHandler(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ingestionKeys.runs() });
      queryClient.invalidateQueries({ queryKey: ingestionKeys.status() });
    },
  });
};

export const useTriggerTeamSync = (): UseMutationResult<void, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (sourceName: string) => {
      await ingestionClient.triggerTeamSync({ sourceName });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ingestionKeys.runs() });
    },
  });
};
