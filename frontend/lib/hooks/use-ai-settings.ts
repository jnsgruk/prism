import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type { AiProvider } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type {
  AiSettings,
  ListAiModelsResponse,
  RefreshModelCatalogueResponse,
  SetProviderSecretResponse,
  TestProviderResponse,
  UpdateAiSettingsResponse,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const aiKeys = {
  all: ["ai"] as const,
  settings: () => [...aiKeys.all, "settings"] as const,
  models: (provider: string, capability: string) => [...aiKeys.all, "models", provider, capability] as const,
  usage: (days: number) => [...aiKeys.all, "usage", days] as const,
};

export const useAiSettings = (): UseQueryResult<AiSettings | undefined> =>
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
  { provider: AiProvider; secretValue: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (req) => client.setProviderSecret(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: aiKeys.settings() });
      // Server auto-triggers catalogue refresh; invalidate models after a delay
      // so the UI picks up the refreshed list.
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: [...aiKeys.all, "models"] });
      }, 5_000);
    },
  });
};

export const useTestProvider = (): UseMutationResult<TestProviderResponse, Error, { provider: AiProvider }> =>
  useMutation({
    mutationFn: (req) => client.testProvider(req),
  });

export const useAiModels = (provider?: AiProvider, capability: string = ""): UseQueryResult<ListAiModelsResponse> =>
  useQuery({
    queryKey: aiKeys.models(String(provider ?? ""), capability),
    queryFn: () => client.listAiModels({ provider, capability }),
    staleTime: 5 * 60 * 1_000,
  });

export const useRefreshModelCatalogue = (): UseMutationResult<RefreshModelCatalogueResponse, Error, void> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => client.refreshModelCatalogue({}),
    onSuccess: () => {
      // Delay invalidation to give the Restate handler time to complete.
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: [...aiKeys.all, "models"] });
      }, 3_000);
    },
  });
};
