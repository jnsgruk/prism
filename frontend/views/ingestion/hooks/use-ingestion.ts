import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  CancelHandlerRunResponse,
  CancelRunResponse,
  GetStatusResponse,
  HandlerRun,
  SourceStatus,
  TriggerBackfillResponse,
  TriggerHandlerResponse,
  TriggerRunResponse,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { HandlersService } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { transport } from "@ps/api/transport";

type RefetchInterval =
  | number
  | false
  | ((query: { state: { data: GetStatusResponse | undefined } }) => number | false);

const handlersClient = createClient(HandlersService, transport);

export const handlersKeys = {
  all: ["handlers"] as const,
  status: (): readonly ["handlers", "status"] => [...handlersKeys.all, "status"] as const,
  runs: (sourceName?: string, handlerName?: string) =>
    [...handlersKeys.all, "runs", sourceName, handlerName] as const,
  handlers: (): readonly ["handlers", "handlers"] => [...handlersKeys.all, "handlers"] as const,
};

export const useIngestionStatus = (options?: {
  refetchInterval?: RefetchInterval;
}): UseQueryResult<SourceStatus[], Error> =>
  useQuery({
    queryKey: handlersKeys.status(),
    queryFn: () => handlersClient.getStatus({}),
    select: (data): SourceStatus[] => data.sources,
    refetchInterval: options?.refetchInterval,
  });

export const useListRuns = (
  sourceName?: string,
  options?: { refetchInterval?: number | false; handlerName?: string; ingestionOnly?: boolean },
): UseQueryResult<HandlerRun[], Error> =>
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

export const useTriggerRun = (): UseMutationResult<TriggerRunResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (sourceName: string) => handlersClient.triggerRun({ sourceName }),
    onSuccess: async () => {
      await queryClient.refetchQueries({ queryKey: handlersKeys.status() });
      await queryClient.refetchQueries({ queryKey: handlersKeys.runs() });
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
      handlersClient.triggerBackfill(req),
    onSuccess: async () => {
      await queryClient.refetchQueries({ queryKey: handlersKeys.status() });
      await queryClient.refetchQueries({ queryKey: handlersKeys.runs() });
    },
  });
};

export const useCancelRun = (): UseMutationResult<CancelRunResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (sourceName: string) => handlersClient.cancelRun({ sourceName }),
    onSuccess: async () => {
      await queryClient.refetchQueries({ queryKey: handlersKeys.status() });
      await queryClient.refetchQueries({ queryKey: handlersKeys.runs() });
    },
  });
};

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

export const useCancelHandlerRun = (): UseMutationResult<
  CancelHandlerRunResponse,
  Error,
  string
> => {
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
