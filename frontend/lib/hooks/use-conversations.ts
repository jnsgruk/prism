import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  ConversationSummary,
  GetConversationResponse,
  GetWorkspaceFileResponse,
  ListWorkspaceFilesResponse,
  ResolvedWorkspaceFile,
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
  workspaceResolution: (id: string) => [...conversationKeys.all, "workspaceResolution", id] as const,
  workspaceFiles: (id: string) => [...conversationKeys.all, "workspaceFiles", id] as const,
};

export const useListConversations = (
  page = 1,
  pageSize = 25,
): UseQueryResult<{ conversations: ConversationSummary[]; totalCount: number }> =>
  useQuery({
    queryKey: [...conversationKeys.list(), page, pageSize],
    queryFn: () => client.listConversations({ page, pageSize }),
    select: (data) => ({
      conversations: data.conversations,
      totalCount: data.totalCount,
    }),
  });

export const useGetConversation = (conversationId: string): UseQueryResult<GetConversationResponse> =>
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

export const useListWorkspaceFiles = (conversationId: string): UseQueryResult<ListWorkspaceFilesResponse> =>
  useQuery({
    queryKey: conversationKeys.workspaceFiles(conversationId),
    queryFn: () => client.listWorkspaceFiles({ conversationId }),
    enabled: !!conversationId,
    refetchInterval: 10_000,
  });

export const useResolveWorkspaceFile = (conversationId: string, path: string): UseQueryResult<ResolvedWorkspaceFile> =>
  useQuery({
    queryKey: [...conversationKeys.workspaceResolution(conversationId), path],
    queryFn: async () => {
      const response = await client.resolveWorkspaceFiles({ conversationId, paths: [path] });
      const file = response.files.find((entry) => entry.path === path);
      if (!file) throw new Error("Could not verify file");
      return file;
    },
    enabled: !!conversationId && !!path,
    staleTime: 30_000,
    retry: false,
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
      const chunks: BlobPart[] = [];
      let contentType = "application/octet-stream";
      let totalSizeBytes = 0;
      let receivedBytes = 0;
      let receivedMetadata = false;

      for await (const response of client.downloadWorkspaceFile(req)) {
        if (response.contentType) {
          contentType = response.contentType;
        }
        if (!receivedMetadata) {
          totalSizeBytes = Number(response.totalSizeBytes);
          receivedMetadata = true;
        }
        if (response.data.length > 0) {
          chunks.push(new Uint8Array(response.data));
          receivedBytes += response.data.length;
        }
      }

      if (!receivedMetadata || receivedBytes !== totalSizeBytes) {
        throw new Error("File download was incomplete. Please retry.");
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
