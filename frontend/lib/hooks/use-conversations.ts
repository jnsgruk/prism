import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  ConversationSummary,
  GetConversationResponse,
  GetWorkspaceFileResponse,
  ListWorkspaceFilesResponse,
  SaveInsightFromConversationResponse,
  UploadWorkspaceFileResponse,
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

/** Result of a streamed workspace file download. */
export interface DownloadedFile {
  blobUrl: string;
  contentType: string;
  totalSizeBytes: number;
}

/**
 * Download a workspace file via the streaming RPC, collecting chunks into a
 * Blob and returning a blob URL suitable for preview or download.
 */
export const useDownloadWorkspaceFile = (): UseMutationResult<
  DownloadedFile,
  Error,
  { conversationId: string; path: string }
> =>
  useMutation({
    mutationFn: async (req: { conversationId: string; path: string }): Promise<DownloadedFile> => {
      const chunks: ArrayBuffer[] = [];
      let contentType = "application/octet-stream";
      let totalSizeBytes = 0;

      for await (const response of client.downloadWorkspaceFile(req)) {
        if (response.contentType) {
          contentType = response.contentType;
        }
        if (response.totalSizeBytes) {
          totalSizeBytes = Number(response.totalSizeBytes);
        }
        if (response.data.length > 0) {
          chunks.push(
            response.data.buffer.slice(
              response.data.byteOffset,
              response.data.byteOffset + response.data.byteLength,
            ) as ArrayBuffer,
          );
        }
      }

      const blob = new Blob(chunks, { type: contentType });
      const blobUrl = URL.createObjectURL(blob);
      return { blobUrl, contentType, totalSizeBytes };
    },
  });

export const useUploadWorkspaceFile = (): UseMutationResult<
  UploadWorkspaceFileResponse,
  Error,
  { conversationId: string; path: string; file: File }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (params: { conversationId: string; path: string; file: File }) => {
      const buffer = await params.file.arrayBuffer();
      return client.uploadWorkspaceFile({
        conversationId: params.conversationId,
        path: params.path,
        contentType: params.file.type || "application/octet-stream",
        data: new Uint8Array(buffer),
      });
    },
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: conversationKeys.workspaceFiles(variables.conversationId),
      });
    },
  });
};
