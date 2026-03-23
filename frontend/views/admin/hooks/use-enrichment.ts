import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type { EnrichmentType } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type {
  DeleteEnrichmentsByTypeResponse,
  Enrichment,
  GetEnrichmentPipelineStatusResponse,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const enrichmentKeys = {
  all: ["enrichment"] as const,
  pipelineStatus: () => [...enrichmentKeys.all, "pipeline-status"] as const,
  forContribution: (id: string) => [...enrichmentKeys.all, "contribution", id] as const,
  forContributions: (ids: string[]) => [...enrichmentKeys.all, "contributions", ...ids] as const,
};

export const useEnrichmentPipelineStatus = (): UseQueryResult<
  GetEnrichmentPipelineStatusResponse,
  Error
> =>
  useQuery({
    queryKey: enrichmentKeys.pipelineStatus(),
    queryFn: () => client.getEnrichmentPipelineStatus({}),
    refetchInterval: 60_000,
  });

export const useEnrichments = (contributionId: string): UseQueryResult<Enrichment[], Error> =>
  useQuery({
    queryKey: enrichmentKeys.forContribution(contributionId),
    queryFn: () => client.getEnrichments({ contributionId }),
    select: (data) => data.enrichments,
    enabled: !!contributionId,
  });

export const useEnrichmentsByContributions = (
  contributionIds: string[],
): UseQueryResult<Enrichment[], Error> =>
  useQuery({
    queryKey: enrichmentKeys.forContributions(contributionIds),
    queryFn: () => client.getEnrichmentsByContributions({ contributionIds }),
    select: (data) => data.enrichments,
    enabled: contributionIds.length > 0,
  });

export const useDeleteEnrichmentsByType = (): UseMutationResult<
  DeleteEnrichmentsByTypeResponse,
  Error,
  { enrichmentType: EnrichmentType }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (req) => client.deleteEnrichmentsByType(req),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: enrichmentKeys.pipelineStatus(),
      });
      queryClient.invalidateQueries({ queryKey: enrichmentKeys.all });
    },
  });
};
