import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  ConversationSummary,
  GetArtifactDownloadUrlResponse,
  GetConversationResponse,
  SaveInsightFromConversationResponse,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const conversationKeys = {
  all: ["conversations"] as const,
  list: () => [...conversationKeys.all, "list"] as const,
  detail: (id: string) => [...conversationKeys.all, "detail", id] as const,
};

export const useListConversations = (
  page = 1,
  pageSize = 25,
): UseQueryResult<{ conversations: ConversationSummary[]; totalCount: number }, Error> =>
  useQuery({
    queryKey: [...conversationKeys.list(), page, pageSize],
    queryFn: () => client.listConversations({ page, pageSize }),
    select: (data) => ({
      conversations: data.conversations,
      totalCount: data.totalCount,
    }),
  });

export const useGetConversation = (
  conversationId: string,
): UseQueryResult<GetConversationResponse, Error> =>
  useQuery({
    queryKey: conversationKeys.detail(conversationId),
    queryFn: () => client.getConversation({ conversationId }),
    enabled: !!conversationId,
  });

export const useGetArtifactDownloadUrl = (): UseMutationResult<
  GetArtifactDownloadUrlResponse,
  Error,
  string
> =>
  useMutation({
    mutationFn: (artifactId: string) => client.getArtifactDownloadUrl({ artifactId }),
  });

export const useDeleteConversation = (): UseMutationResult<object, Error, string> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (conversationId: string) => client.deleteConversation({ conversationId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
    },
  });
};

export const useRenameConversation = (): UseMutationResult<
  object,
  Error,
  { conversationId: string; title: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { conversationId: string; title: string }) => client.renameConversation(req),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
      queryClient.invalidateQueries({
        queryKey: conversationKeys.detail(variables.conversationId),
      });
    },
  });
};

export const useSaveInsightFromConversation = (): UseMutationResult<
  SaveInsightFromConversationResponse,
  Error,
  { conversationId: string; messageId: string; title: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { conversationId: string; messageId: string; title: string }) =>
      client.saveInsightFromConversation(req),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: conversationKeys.detail(variables.conversationId),
      });
    },
  });
};
