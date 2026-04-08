import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  ConversationSummary,
  GetConversationResponse,
  GetWorkspaceFileResponse,
  ListWorkspaceFilesResponse,
  SaveInsightFromConversationResponse,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const conversationKeys = {
  all: ["conversations"] as const,
  list: () => [...conversationKeys.all, "list"] as const,
  detail: (id: string) => [...conversationKeys.all, "detail", id] as const,
  workspaceFiles: (id: string) => [...conversationKeys.all, "workspaceFiles", id] as const,
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

export const useListWorkspaceFiles = (
  conversationId: string,
): UseQueryResult<ListWorkspaceFilesResponse, Error> =>
  useQuery({
    queryKey: conversationKeys.workspaceFiles(conversationId),
    queryFn: () => client.listWorkspaceFiles({ conversationId }),
    enabled: !!conversationId,
    refetchInterval: 10_000,
  });

export const useGetWorkspaceFile = (): UseMutationResult<
  GetWorkspaceFileResponse,
  Error,
  { conversationId: string; path: string }
> =>
  useMutation({
    mutationFn: (req: { conversationId: string; path: string }) => client.getWorkspaceFile(req),
  });
