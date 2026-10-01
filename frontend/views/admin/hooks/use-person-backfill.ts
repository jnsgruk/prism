import { pipelineKeys } from "@/views/ingestion/hooks/use-pipeline";
import { createClient } from "@connectrpc/connect";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { toast } from "sonner";

import { HandlersService } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import type {
  PersonBackfillCapabilities,
  PipelineRunSummary,
  PipelineInfo,
  TriggerPipelineResponse,
  CancelPipelineResponse,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { transport } from "@ps/api/transport";
import { isActivePipeline } from "@ps/pipeline-status";

const client = createClient(HandlersService, transport);

export const personBackfillKeys = {
  person: (personId: string) => ["pipeline", "person", personId] as const,
  history: (personId: string) => [...personBackfillKeys.person(personId), "history"] as const,
  status: (personId: string, pipelineId?: string) =>
    [...personBackfillKeys.person(personId), "status", pipelineId] as const,
  capabilities: ["pipeline", "person-capabilities"] as const,
};

export const usePersonBackfillCapabilities = (
  enabled: boolean,
): UseQueryResult<PersonBackfillCapabilities | undefined> =>
  useQuery({
    queryKey: personBackfillKeys.capabilities,
    queryFn: () => client.getStatus({}),
    enabled,
    select: (data) => data.personBackfillCapabilities,
  });

export const usePersonBackfillHistory = (personId: string, enabled: boolean): UseQueryResult<PipelineRunSummary[]> =>
  useQuery({
    queryKey: personBackfillKeys.history(personId),
    queryFn: () => client.listPipelineRuns({ personId }),
    select: (data) => data.pipelines.filter((summary) => summary.pipeline?.personId === personId),
    enabled,
    refetchInterval: (query) => {
      if (!enabled) return false;
      return query.state.data?.pipelines.some((summary) => isActivePipeline(summary.pipeline?.status)) ? 3000 : 30000;
    },
  });

export const usePersonBackfillStatus = (
  personId: string,
  pipelineId?: string,
  enabled = true,
): UseQueryResult<PipelineInfo | undefined> =>
  useQuery({
    queryKey: personBackfillKeys.status(personId, pipelineId),
    queryFn: () => client.getPipelineStatus({ personId, pipelineId }),
    select: (data) =>
      data.current?.personId === personId && data.current.id === pipelineId ? data.current : undefined,
    enabled: enabled && !!pipelineId,
    refetchInterval: (query) =>
      enabled && pipelineId && (!query.state.data?.current || isActivePipeline(query.state.data.current.status))
        ? 3000
        : false,
  });

export const useLaunchPersonBackfill = (
  personId: string,
): UseMutationResult<
  TriggerPipelineResponse,
  Error,
  { sourceIds: string[]; sinceDate: string; submissionId: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { sourceIds: string[]; sinceDate: string; submissionId: string }) =>
      client.triggerPipeline({
        sinceDate: args.sinceDate,
        scope: { personId, sourceIds: args.sourceIds },
        submissionId: args.submissionId,
      }),
    onSuccess: () => {
      toast.success("Backfill accepted");
      void queryClient.invalidateQueries({ queryKey: pipelineKeys.all });
    },
    onError: (error) => toast.error(error.message),
  });
};

export const useCancelPersonBackfill = (personId: string): UseMutationResult<CancelPipelineResponse, Error, string> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (pipelineId: string) => client.cancelPipeline({ pipelineId }),
    onSuccess: () => {
      toast.success("Backfill cancellation requested");
      void queryClient.invalidateQueries({ queryKey: personBackfillKeys.person(personId) });
      void queryClient.invalidateQueries({ queryKey: pipelineKeys.status() });
    },
    onError: (error) => toast.error(error.message),
  });
};
