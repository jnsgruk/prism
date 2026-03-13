import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  GetStatusResponse,
  IngestionRun,
  SourceStatus,
  TriggerBackfillResponse,
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
  runs: (sourceName?: string): readonly ["ingestion", "runs", string | undefined] =>
    [...ingestionKeys.all, "runs", sourceName] as const,
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
  options?: { refetchInterval?: number | false },
): UseQueryResult<IngestionRun[], Error> =>
  useQuery({
    queryKey: ingestionKeys.runs(sourceName),
    queryFn: () => ingestionClient.listRuns({ sourceName }),
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
