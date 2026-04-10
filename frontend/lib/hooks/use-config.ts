import type { JsonObject } from "@bufbuild/protobuf";
import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type {
  CreateSourceResponse,
  DeleteSourceResponse,
  SetSecretResponse,
  SourceConfig,
  TestConnectionResponse,
  UpdateSourceResponse,
} from "@ps/api/gen/canonical/prism/v1/config_pb";
import { ConfigService } from "@ps/api/gen/canonical/prism/v1/config_pb";
import { transport } from "@ps/api/transport";

const configClient = createClient(ConfigService, transport);

export const configKeys = {
  all: ["config"] as const,
  sources: (): readonly ["config", "sources"] => [...configKeys.all, "sources"] as const,
  source: (sourceId: string): readonly ["config", "source", string] => [...configKeys.all, "source", sourceId] as const,
};

export const useListSources = (): UseQueryResult<SourceConfig[], Error> =>
  useQuery({
    queryKey: configKeys.sources(),
    queryFn: () => configClient.listSources({}),
    select: (data): SourceConfig[] => data.sources,
  });

export const useGetSource = (sourceId: string): UseQueryResult<SourceConfig | undefined, Error> =>
  useQuery({
    queryKey: configKeys.source(sourceId),
    queryFn: () => configClient.getSource({ sourceId }),
    select: (data): SourceConfig | undefined => data.source,
    enabled: !!sourceId,
  });

export const useCreateSource = (): UseMutationResult<
  CreateSourceResponse,
  Error,
  { sourceType: Platform; name: string; settings?: JsonObject; scheduleCron?: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceType: Platform; name: string; settings?: JsonObject; scheduleCron?: string }) =>
      configClient.createSource(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
      queryClient.invalidateQueries({ queryKey: ["ingestion", "status"] });
    },
  });
};

export const useUpdateSource = (): UseMutationResult<
  UpdateSourceResponse,
  Error,
  { sourceId: string; enabled?: boolean; settings?: JsonObject; scheduleCron?: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceId: string; enabled?: boolean; settings?: JsonObject; scheduleCron?: string }) =>
      configClient.updateSource(req),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
      queryClient.invalidateQueries({ queryKey: configKeys.source(variables.sourceId) });
      // When toggling enabled, the handlers status (SourceStatus[]) changes too
      if (variables.enabled !== undefined) {
        queryClient.invalidateQueries({ queryKey: ["handlers", "status"] });
      }
    },
  });
};

export const useDeleteSource = (): UseMutationResult<DeleteSourceResponse, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (sourceId: string) => configClient.deleteSource({ sourceId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
      queryClient.invalidateQueries({ queryKey: ["ingestion", "status"] });
    },
  });
};

export const useSetSecret = (): UseMutationResult<
  SetSecretResponse,
  Error,
  { sourceId: string; secretKey: string; secretValue: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceId: string; secretKey: string; secretValue: string }) => configClient.setSecret(req),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: configKeys.source(variables.sourceId) });
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
    },
  });
};

export const useTestConnection = (): UseMutationResult<TestConnectionResponse, Error, string> =>
  useMutation({
    mutationFn: (sourceId: string) => configClient.testConnection({ sourceId }),
  });
