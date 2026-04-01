import { Loader2, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import type { ConversationMessage } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { useAiModels } from "@/views/admin/hooks/use-ai-settings";
import { useAskQuestion, type ContextUsage } from "@/views/ask/hooks/use-ask-question";
import { useGetConversation } from "@/views/ask/hooks/use-conversations";
import { ConversationThread } from "@/views/ask/components/conversation-thread";
import { QueryInput } from "@/views/ask/components/query-input";
import { SuggestedQuestions } from "@/views/ask/components/suggested-questions";

const AskPage = (): React.ReactElement => {
  const { conversationId } = useParams<{ conversationId?: string }>();
  const navigate = useNavigate();
  const { state, ask, cancel, reset, resume } = useAskQuestion();
  const [selectedModel, setSelectedModel] = useState<string | undefined>();

  const { data: conversationData, isLoading } = useGetConversation(conversationId ?? "");

  const messages: ConversationMessage[] = useMemo(
    () => conversationData?.messages ?? [],
    [conversationData],
  );

  const conversationArtifacts = useMemo(
    () => conversationData?.artifacts ?? [],
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

  const handleAsk = useCallback(
    (question: string): void => {
      ask(question, conversationId, selectedModel);
    },
    [ask, conversationId, selectedModel],
  );

  const isActive = state.status === "streaming" || state.status === "container_starting";

  // Resolve context usage: prefer live streaming data, fall back to stored conversation totals.
  // Use a ref to persist the last known value so it doesn't disappear between turns.
  const conv = conversationData?.conversation;
  const { data: modelsResponse } = useAiModels(undefined, "tool_use");
  const lastContextUsage = useRef<ContextUsage | undefined>(undefined);
  const contextUsage = useMemo((): ContextUsage | undefined => {
    let usage: ContextUsage | undefined;
    // Live streaming data takes priority.
    if (state.status === "streaming" || state.status === "completed") {
      usage = state.contextUsage;
    }
    // Fall back to stored conversation totals.
    if (!usage && conv && (conv.totalPromptTokens > 0 || conv.totalCompletionTokens > 0)) {
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
  }, [state, conv, modelsResponse]);

  const showSuggestions = !conversationId && state.status === "idle" && messages.length === 0;

  const headerActions = conversationId ? (
    <Button
      variant="outline"
      size="sm"
      className="gap-1.5"
      onClick={() => {
        reset();
        navigate("/ask");
      }}
    >
      <Plus className="size-4" />
      New
    </Button>
  ) : null;

  return (
    <>
      <PageHeader title="Ask" actions={headerActions} />
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
                    conversationArtifacts={conversationArtifacts}
                    onRetry={handleAsk}
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
              />
            </div>
          </>
        )}
      </div>
    </>
  );
};

export default AskPage;
