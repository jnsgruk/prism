import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  AiSettings,
  SetProviderSecretResponse,
  TestProviderResponse,
  UpdateAiSettingsResponse,
} from "@ps/api/gen/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const aiKeys = {
  all: ["ai"] as const,
  settings: () => [...aiKeys.all, "settings"] as const,
  cost: (days: number) => [...aiKeys.all, "cost", days] as const,
  storageHealth: () => [...aiKeys.all, "storage-health"] as const,
};

export const useAiSettings = (): UseQueryResult<AiSettings | undefined, Error> =>
  useQuery({
    queryKey: aiKeys.settings(),
    queryFn: () => client.getAiSettings({}),
    select: (data) => data.settings,
  });

export const useUpdateAiSettings = (): UseMutationResult<
  UpdateAiSettingsResponse,
  Error,
  Parameters<typeof client.updateAiSettings>[0]
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (req) => client.updateAiSettings(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: aiKeys.settings() });
    },
  });
};

export const useSetProviderSecret = (): UseMutationResult<
  SetProviderSecretResponse,
  Error,
  { provider: string; secretValue: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (req) => client.setProviderSecret(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: aiKeys.settings() });
    },
  });
};

export const useTestProvider = (): UseMutationResult<
  TestProviderResponse,
  Error,
  { provider: string }
> =>
  useMutation({
    mutationFn: (req) => client.testProvider(req),
  });

export const useStorageHealth = (): UseQueryResult<
  Awaited<ReturnType<typeof client.getStorageHealth>>,
  Error
> =>
  useQuery({
    queryKey: aiKeys.storageHealth(),
    queryFn: () => client.getStorageHealth({}),
    refetchInterval: 60_000,
  });
