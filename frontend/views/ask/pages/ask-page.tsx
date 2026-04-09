import { Loader2, PanelRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import type { ConversationMessage } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { MentionType } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import type { MentionItem } from "@/views/ask/hooks/use-mention-picker";
import { useAiModels } from "@/lib/hooks/use-ai-settings";
import { useDownloadWorkspaceFile, useUploadWorkspaceFile } from "@/lib/hooks/use-conversations";
import { useAskQuestion, type ContextUsage } from "@/views/ask/hooks/use-ask-question";
import { useGetConversation } from "@/lib/hooks/use-conversations";
import { isTextContent } from "@/views/ask/hooks/use-file-tree";
import type { PreviewState } from "@/views/ask/components/workspace-preview";
import { WorkspacePreviewDialog } from "@/views/ask/components/workspace-preview-dialog";
import type { AttachedFileInfo } from "@/views/ask/components/file-attachment-chips";
import { ConversationThread } from "@/views/ask/components/conversation-thread";
import { QueryInput } from "@/views/ask/components/query-input";
import { SuggestedQuestions } from "@/views/ask/components/suggested-questions";
import { WorkspaceSidebar } from "@/views/ask/components/workspace-sidebar";
import { toast } from "sonner";

const AskPage = (): React.ReactElement => {
  const { conversationId } = useParams<{ conversationId?: string }>();
  const navigate = useNavigate();
  const { state, ask, cancel, reset, resume } = useAskQuestion();
  const [selectedModel, setSelectedModel] = useState<string | undefined>();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [pendingFiles, setPendingFiles] = useState<File[]>([]);
  const uploadFile = useUploadWorkspaceFile();
  const downloadFile = useDownloadWorkspaceFile();
  const [filePreview, setFilePreview] = useState<PreviewState | null>(null);
  const [filePreviewOpen, setFilePreviewOpen] = useState(false);
  const previewBlobRef = useRef<string | null>(null);
  // Track files submitted with the current question so they appear immediately
  // in the optimistic user message (before the DB message is loaded).
  const [submittedFiles, setSubmittedFiles] = useState<string[]>([]);

  const { data: conversationData, isLoading } = useGetConversation(conversationId ?? "");

  const messages: ConversationMessage[] = useMemo(
    () => conversationData?.messages ?? [],
    [conversationData],
  );

  // Track the conversation ID that the current stream created so that
  // navigating from /ask → /ask/{id} mid-stream does NOT trigger a reset.
  const streamConvIdRef = useRef<string | undefined>(undefined);

  // Navigate to the conversation URL as soon as we learn the conversation ID
  // (from the conversationCreated event), not just on completion.
  const stateConversationId =
    state.status !== "idle" && state.status !== "error" ? state.conversationId : undefined;
  useEffect(() => {
    if (!conversationId && stateConversationId) {
      streamConvIdRef.current = stateConversationId;
      navigate(`/ask/${stateConversationId}`, { replace: true });
    }
  }, [stateConversationId, conversationId, navigate]);

  // Reset state when the user navigates to a different conversation, but NOT
  // when we just navigated to the conversation the current stream created.
  useEffect(() => {
    if (conversationId === streamConvIdRef.current) {
      streamConvIdRef.current = undefined;
      return;
    }
    reset();
  }, [conversationId, reset]);

  // Auto-resume if we navigate to a conversation with an active query.
  const queryStatus = conversationData?.conversation?.queryStatus;
  const hasResumed = useRef(false);

  useEffect(() => {
    if (
      conversationId &&
      (queryStatus === "running" || queryStatus === "pending") &&
      state.status === "idle" &&
      !hasResumed.current
    ) {
      hasResumed.current = true;
      const lastQuestion = messages.findLast((m) => m.role === "user")?.content;
      resume(conversationId, lastQuestion);
    }
  }, [conversationId, queryStatus, state.status, resume, messages]);

  // Reset the resume guard when conversation changes.
  useEffect(() => {
    hasResumed.current = false;
  }, [conversationId]);

  // Generate a stable conversation ID for uploads when creating a new conversation.
  const pendingConvIdRef = useRef<string | undefined>(undefined);

  const handleAsk = useCallback(
    async (question: string, mentionItems?: MentionItem[]): Promise<void> => {
      const effectiveConvId = conversationId ?? pendingConvIdRef.current ?? crypto.randomUUID();
      if (!conversationId) {
        pendingConvIdRef.current = effectiveConvId;
      }

      let attachedFilePaths: string[] = [];
      if (pendingFiles.length > 0) {
        const results = await Promise.allSettled(
          pendingFiles.map((file) =>
            uploadFile.mutateAsync({
              conversationId: effectiveConvId,
              path: `uploads/${file.name}`,
              file,
            }),
          ),
        );
        attachedFilePaths = results
          .filter(
            (r): r is PromiseFulfilledResult<Awaited<ReturnType<typeof uploadFile.mutateAsync>>> =>
              r.status === "fulfilled",
          )
          .map((r) => r.value.file?.path ?? "")
          .filter(Boolean);

        const failures = results.filter((r) => r.status === "rejected");
        if (failures.length > 0) {
          toast.error(`${failures.length} file${failures.length > 1 ? "s" : ""} failed to upload`);
        }
        setPendingFiles([]);
      }

      // Separate file mentions from people/team mentions.
      const fileMentionPaths = (mentionItems ?? [])
        .filter((m) => m.type === "file")
        .map((m) => m.id);
      const allFilePaths = [...attachedFilePaths, ...fileMentionPaths];

      // Build proto Mention objects for all @-mentions.
      const typeMap = {
        file: MentionType.FILE,
        person: MentionType.PERSON,
        team: MentionType.TEAM,
      } as const;
      const protoMentions = (mentionItems ?? []).map((m) => ({
        id: m.id,
        name: m.name,
        type: typeMap[m.type],
      }));

      // Store the submitted files so the optimistic user message can show them
      setSubmittedFiles(allFilePaths);

      pendingConvIdRef.current = undefined;
      ask(
        question,
        effectiveConvId,
        selectedModel,
        allFilePaths.length > 0 ? allFilePaths : undefined,
        protoMentions.length > 0 ? protoMentions : undefined,
      );
    },
    [ask, conversationId, selectedModel, pendingFiles, uploadFile],
  );

  const handleFilesAdded = useCallback((files: File[]) => {
    setPendingFiles((prev) => [...prev, ...files]);
  }, []);

  const handleFileRemoved = useCallback((index: number) => {
    setPendingFiles((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handleFileClick = useCallback(
    (path: string) => {
      if (!conversationId) return;
      if (previewBlobRef.current) {
        URL.revokeObjectURL(previewBlobRef.current);
        previewBlobRef.current = null;
      }
      downloadFile.mutate(
        { conversationId, path },
        {
          onSuccess: async ({ blobUrl, contentType }) => {
            previewBlobRef.current = blobUrl;
            let textContent: string | undefined;
            if (isTextContent(contentType)) {
              const res = await fetch(blobUrl);
              textContent = await res.text();
            }
            const name = path.split("/").pop() ?? path;
            setFilePreview({
              artifact: { id: path, displayName: name, contentType, sizeBytes: 0 },
              url: blobUrl,
              contentType,
              textContent,
            });
            setFilePreviewOpen(true);
          },
          onError: (err) => toast.error(`Failed to preview: ${err.message}`),
        },
      );
    },
    [conversationId, downloadFile],
  );

  const attachedFileInfos: AttachedFileInfo[] = useMemo(
    () => pendingFiles.map((f) => ({ name: f.name, size: f.size })),
    [pendingFiles],
  );

  const isActive =
    state.status === "streaming" ||
    state.status === "container_starting" ||
    state.status === "cancelling";

  // Resolve context usage: prefer live streaming data, fall back to stored conversation totals.
  // Only show when the pod is active — once reaped, the context is gone and a new
  // query will start a fresh pod with an empty context window.
  const conv = conversationData?.conversation;
  const podActive = conv?.containerStatus === "active";
  const { data: modelsResponse } = useAiModels(undefined, "tool_use");
  const lastContextUsage = useRef<ContextUsage | undefined>(undefined);
  const contextUsage = useMemo((): ContextUsage | undefined => {
    if (!podActive && state.status !== "streaming" && state.status !== "completed") {
      lastContextUsage.current = undefined;
      return undefined;
    }
    let usage: ContextUsage | undefined;
    // Live streaming data takes priority.
    if (state.status === "streaming" || state.status === "completed") {
      usage = state.contextUsage;
    }
    // Fall back to stored conversation totals only while pod is active.
    if (
      !usage &&
      podActive &&
      conv &&
      (conv.totalPromptTokens > 0 || conv.totalCompletionTokens > 0)
    ) {
      const modelId = conv.modelName.split("/").slice(1).join("/");
      const model = modelsResponse?.models?.find((m) => m.id === modelId);
      usage = {
        inputTokens: conv.totalPromptTokens,
        outputTokens: conv.totalCompletionTokens,
        contextWindow: model?.contextLength ?? 0,
      };
    }
    if (usage) lastContextUsage.current = usage;
    return usage ?? lastContextUsage.current;
  }, [state, conv, modelsResponse, podActive]);

  const showSuggestions = !conversationId && state.status === "idle" && messages.length === 0;

  const headerActions = (
    <Button
      variant={sidebarOpen ? "default" : "outline"}
      size="icon"
      className="size-8"
      onClick={() => setSidebarOpen((v) => !v)}
      title="Toggle workspace"
    >
      <PanelRight className="size-4" />
    </Button>
  );

  return (
    <>
      <PageHeader title="Ask" actions={headerActions} />
      <div className="flex min-w-0 flex-1 overflow-hidden">
        {/* Main conversation column */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {isLoading && conversationId ? (
            <div className="flex flex-1 items-center justify-center">
              <Loader2 className="size-6 animate-spin text-muted-foreground" />
            </div>
          ) : (
            <>
              <div className="flex-1 overflow-y-auto px-6 pt-6">
                {showSuggestions ? (
                  <SuggestedQuestions onSelect={handleAsk} />
                ) : (
                  <div className="mx-auto max-w-3xl">
                    <ConversationThread
                      messages={messages}
                      state={state}
                      onRetry={handleAsk}
                      conversationId={conversationId}
                      onFileClick={handleFileClick}
                      submittedFiles={submittedFiles}
                    />
                  </div>
                )}
              </div>
              <div className="mx-auto w-full max-w-3xl px-6 pb-6 pt-3">
                <QueryInput
                  onSubmit={handleAsk}
                  onCancel={cancel}
                  isStreaming={isActive}
                  disabled={isLoading}
                  selectedModel={selectedModel}
                  onModelChange={setSelectedModel}
                  contextUsage={contextUsage}
                  containerStatus={conv?.containerStatus}
                  podName={conv?.containerPodName}
                  podIp={conv?.containerPodIp}
                  attachedFiles={attachedFileInfos}
                  onFilesAdded={handleFilesAdded}
                  onFileRemoved={handleFileRemoved}
                  conversationId={conversationId}
                />
              </div>
            </>
          )}
        </div>

        {/* Workspace file sidebar */}
        <WorkspaceSidebar
          open={sidebarOpen}
          conversationId={conversationId}
          onClose={() => setSidebarOpen(false)}
        />
      </div>

      {/* Preview dialog for clicking attached files */}
      <WorkspacePreviewDialog
        state={filePreview}
        open={filePreviewOpen}
        onOpenChange={setFilePreviewOpen}
        onDownload={() => {
          if (filePreview?.url) {
            const a = document.createElement("a");
            a.href = filePreview.url;
            a.download = filePreview.artifact.displayName;
            a.click();
          }
        }}
      />
    </>
  );
};

export default AskPage;
