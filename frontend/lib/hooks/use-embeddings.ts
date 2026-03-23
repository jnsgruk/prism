import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery } from "@tanstack/react-query";

import type {
  FindSimilarResponse,
  GetEmbeddingStatusResponse,
  SearchByTextResponse,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const embeddingKeys = {
  all: ["embeddings"] as const,
  similar: (contributionId: string, platform?: string) =>
    [...embeddingKeys.all, "similar", contributionId, platform] as const,
  status: () => [...embeddingKeys.all, "status"] as const,
};

export const useEmbeddingSimilar = (
  contributionId: string,
  options?: {
    limit?: number;
    platform?: string;
    enabled?: boolean;
  },
): UseQueryResult<FindSimilarResponse, Error> =>
  useQuery({
    queryKey: embeddingKeys.similar(contributionId, options?.platform),
    queryFn: () =>
      client.findSimilar({
        contributionId,
        limit: options?.limit ?? 5,
        platform: options?.platform,
      }),
    enabled: options?.enabled !== false && !!contributionId,
  });

export const useEmbeddingSearch = (): ReturnType<
  typeof useMutation<
    SearchByTextResponse,
    Error,
    { queryText: string; limit?: number; platform?: string }
  >
> =>
  useMutation<
    SearchByTextResponse,
    Error,
    { queryText: string; limit?: number; platform?: string }
  >({
    mutationFn: (params) => client.searchByText(params),
  });

export const useEmbeddingStatus = (): UseQueryResult<GetEmbeddingStatusResponse, Error> =>
  useQuery({
    queryKey: embeddingKeys.status(),
    queryFn: () => client.getEmbeddingStatus({}),
    refetchInterval: 30_000,
  });
