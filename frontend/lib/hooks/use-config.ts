import type { JsonObject } from "@bufbuild/protobuf";
import { createClient } from "@connectrpc/connect";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { ConfigService } from "@ps/api/gen/prism/v1/config_pb";
import { transport } from "@ps/api/transport";

const configClient = createClient(ConfigService, transport);

export const configKeys = {
  all: ["config"] as const,
  sources: () => [...configKeys.all, "sources"] as const,
  source: (sourceId: string) => [...configKeys.all, "source", sourceId] as const,
};

export const useListSources = () =>
  useQuery({
    queryKey: configKeys.sources(),
    queryFn: () => configClient.listSources({}),
    select: (data) => data.sources,
  });

export const useGetSource = (sourceId: string) =>
  useQuery({
    queryKey: configKeys.source(sourceId),
    queryFn: () => configClient.getSource({ sourceId }),
    select: (data) => data.source,
    enabled: !!sourceId,
  });

export const useCreateSource = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceType: string; name: string; settings?: JsonObject; scheduleCron?: string }) =>
      configClient.createSource(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
    },
  });
};

export const useUpdateSource = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceId: string; enabled?: boolean; settings?: JsonObject; scheduleCron?: string }) =>
      configClient.updateSource(req),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
      queryClient.invalidateQueries({ queryKey: configKeys.source(variables.sourceId) });
    },
  });
};

export const useDeleteSource = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (sourceId: string) => configClient.deleteSource({ sourceId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
    },
  });
};

export const useSetSecret = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { sourceId: string; secretKey: string; secretValue: string }) => configClient.setSecret(req),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: configKeys.source(variables.sourceId) });
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
    },
  });
};

export const useTestConnection = () =>
  useMutation({
    mutationFn: (sourceId: string) => configClient.testConnection({ sourceId }),
  });
