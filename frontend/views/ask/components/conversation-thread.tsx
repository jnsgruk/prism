import { useEffect, useRef } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { AlertCircle } from "lucide-react";

import type { AgentState } from "@/views/ask/hooks/use-ask-question";
import type {
  ConversationArtifact,
  ConversationMessage,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { AgentResponse } from "./agent-response";
import { ArtifactList } from "./artifact-list";
import { ContainerStatus } from "./container-status";
import { UserMessage } from "./user-message";
import { AnswerContent } from "./answer-content";
import { ThinkingSteps } from "./thinking-steps";

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";

const parseReasoningTrace = (json?: string): AgentStep[] => {
  if (!json) return [];
  try {
    const trace = JSON.parse(json);
    return (trace.steps ?? []).map(
      (s: {
        tool_name: string;
        arguments?: string;
        result_summary?: string;
        duration_ms?: number;
      }) => ({
        kind: "tool" as const,
        toolName: s.tool_name,
        argumentsJson: s.arguments ?? "{}",
        resultSummary: s.result_summary,
        durationMs: s.duration_ms,
        success: true,
        status: "completed" as const,
      }),
    );
  } catch {
    return [];
  }
};

export const ConversationThread = ({
  messages,
  state,
  conversationArtifacts = [],
}: {
  messages: ConversationMessage[];
  state: AgentState;
  conversationArtifacts?: ConversationArtifact[];
}): React.ReactElement => {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, state]);

  return (
    <div className="space-y-6">
      {messages.map((msg) => (
        <div key={msg.id}>
          {msg.role === "user" ? (
            <UserMessage content={msg.content} />
          ) : (
            <div className="flex gap-3">
              <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-violet-100 text-violet-700 dark:bg-violet-900/30 dark:text-violet-400">
                <span className="text-xs font-medium">AI</span>
              </div>
              <div className="min-w-0 flex-1 space-y-2 pt-0.5">
                {msg.reasoningTraceJson && (
                  <ThinkingSteps
                    steps={parseReasoningTrace(msg.reasoningTraceJson)}
                    defaultOpen={false}
                  />
                )}
                <AnswerContent content={msg.content} />
              </div>
            </div>
          )}
        </div>
      ))}

      {conversationArtifacts.length > 0 && state.status === "idle" && (
        <ArtifactList artifacts={conversationArtifacts} />
      )}

      {state.status === "container_starting" && <ContainerStatus message={state.message} />}

      {(state.status === "streaming" || state.status === "completed") && (
        <AgentResponse
          state={state}
          steps={state.steps}
          answer={state.status === "streaming" ? state.partialAnswer : state.answer}
          artifacts={state.artifacts}
          supportingData={state.status === "completed" ? state.supportingData : undefined}
        />
      )}

      {state.status === "error" && (
        <Alert variant="destructive">
          <AlertCircle className="size-4" />
          <AlertDescription>{state.message}</AlertDescription>
        </Alert>
      )}

      <div ref={bottomRef} />
    </div>
  );
};
