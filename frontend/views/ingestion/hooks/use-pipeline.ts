import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import type {
  CancelPipelineResponse,
  GetPipelineStatusResponse,
  PipelineInfo,
  TriggerPipelineResponse,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { HandlersService } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { transport } from "@ps/api/transport";

const client = createClient(HandlersService, transport);

export const pipelineKeys = {
  all: ["pipeline"] as const,
  status: () => [...pipelineKeys.all, "status"] as const,
};

type RefetchInterval =
  | number
  | false
  | ((query: { state: { data: GetPipelineStatusResponse | undefined } }) => number | false);

export const usePipelineStatus = (options?: {
  refetchInterval?: RefetchInterval;
}): UseQueryResult<GetPipelineStatusResponse> =>
  useQuery({
    queryKey: pipelineKeys.status(),
    queryFn: () => client.getPipelineStatus({}),
    refetchInterval: options?.refetchInterval,
  });

/** Convenience accessor for the current pipeline from the status response. */
export const useCurrentPipeline = (options?: {
  refetchInterval?: RefetchInterval;
}): { current: PipelineInfo | undefined; recent: PipelineInfo[]; isLoading: boolean } => {
  const { data, isLoading } = usePipelineStatus(options);
  return {
    current: data?.current,
    recent: data?.recent ?? [],
    isLoading,
  };
};

export const useTriggerPipeline = (): UseMutationResult<
  TriggerPipelineResponse,
  Error,
  { sinceDate?: string } | void
> => {
  const queryClient = useQueryClient();
  const submission = useRef<{ sinceDate?: string; id: string } | undefined>(undefined);

  return useMutation({
    mutationFn: (args) => {
      if (!submission.current || submission.current.sinceDate !== args?.sinceDate) {
        submission.current = { sinceDate: args?.sinceDate, id: crypto.randomUUID() };
      }
      return client.triggerPipeline({ sinceDate: args?.sinceDate, submissionId: submission.current.id });
    },
    onSuccess: async () => {
      submission.current = undefined;
      await queryClient.invalidateQueries({ queryKey: pipelineKeys.all });
    },
  });
};

export const useCancelPipeline = (): UseMutationResult<CancelPipelineResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (pipelineId: string) => client.cancelPipeline({ pipelineId }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: pipelineKeys.all });
    },
  });
};
