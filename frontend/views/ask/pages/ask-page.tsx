import { Loader2, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import type { ConversationMessage } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { useAskQuestion } from "@/views/ask/hooks/use-ask-question";
import { useGetConversation } from "@/views/ask/hooks/use-conversations";
import { ConversationHistory } from "@/views/ask/components/conversation-history";
import { ConversationThread } from "@/views/ask/components/conversation-thread";
import { QueryInput } from "@/views/ask/components/query-input";
import { SuggestedQuestions } from "@/views/ask/components/suggested-questions";

const AskPage = (): React.ReactElement => {
  const { conversationId } = useParams<{ conversationId?: string }>();
  const navigate = useNavigate();
  const { state, ask, cancel, reset } = useAskQuestion();

  const { data: conversationData, isLoading } = useGetConversation(conversationId ?? "");

  const messages: ConversationMessage[] = useMemo(
    () => conversationData?.messages ?? [],
    [conversationData],
  );

  const conversationArtifacts = useMemo(
    () => conversationData?.artifacts ?? [],
    [conversationData],
  );

  useEffect(() => {
    reset();
  }, [conversationId, reset]);

  const handleAsk = useCallback(
    (question: string): void => {
      ask(question, conversationId);
    },
    [ask, conversationId],
  );

  useEffect(() => {
    if (state.status === "completed" && !conversationId && state.conversationId) {
      navigate(`/ask/${state.conversationId}`, { replace: true });
    }
  }, [state, conversationId, navigate]);

  const isActive = state.status === "streaming" || state.status === "container_starting";
  const showSuggestions = !conversationId && state.status === "idle" && messages.length === 0;

  const headerActions = (
    <div className="flex items-center gap-2">
      <ConversationHistory />
      {conversationId && (
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
      )}
    </div>
  );

  return (
    <>
      <PageHeader title="Ask" description="Query your engineering data" actions={headerActions} />
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
              />
            </div>
          </>
        )}
      </div>
    </>
  );
};

export default AskPage;
