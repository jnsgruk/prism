import { useEffect, useRef } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { AlertCircle, Sparkles } from "lucide-react";

import type { AgentState, AgentStep } from "@/views/ask/hooks/use-ask-question";
import type {
  ConversationArtifact,
  ConversationMessage,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { AgentResponse } from "./agent-response";
import { ArtifactList } from "./artifact-list";
import { ContainerStatus } from "./container-status";
import { EvidencePanel } from "./evidence-panel";
import { ThinkingSteps } from "./thinking-steps";
import { UserMessage } from "./user-message";
import { AnswerContent } from "./answer-content";

const parseReasoningTrace = (json?: string): AgentStep[] => {
  if (!json) return [];
  try {
    const trace = JSON.parse(json);
    return (trace.steps ?? []).map(
      (
        s: {
          kind?: string;
          tool_name?: string;
          call_id?: string;
          arguments?: string;
          result_summary?: string;
          duration_ms?: number;
          text?: string;
          part_index?: number;
        },
        i: number,
      ): AgentStep => {
        if (s.kind === "reasoning") {
          return {
            kind: "reasoning" as const,
            text: s.text ?? "",
            partIndex: s.part_index ?? i,
          };
        }
        // Default to tool step (backward compatible with traces without kind field)
        return {
          kind: "tool" as const,
          callId: s.call_id ?? `trace-${i}`,
          toolName: s.tool_name ?? "unknown",
          argumentsJson: s.arguments ?? "{}",
          resultSummary: s.result_summary,
          durationMs: s.duration_ms,
          success: true,
          status: "completed" as const,
        };
      },
    );
  } catch {
    return [];
  }
};

const HistoricalAssistantMessage = ({ msg }: { msg: ConversationMessage }): React.ReactElement => {
  const steps = parseReasoningTrace(msg.reasoningTraceJson);
  const toolCallCount = steps.filter((s) => s.kind === "tool").length;
  const totalTokens = msg.promptTokens + msg.completionTokens;

  return (
    <div className="flex gap-3">
      <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-violet-100 text-violet-700 dark:bg-violet-900/30 dark:text-violet-400">
        <Sparkles className="size-3.5" />
      </div>
      <div className="min-w-0 flex-1 space-y-3 pt-0.5">
        <AnswerContent content={msg.content} />
        {steps.length > 0 && <ThinkingSteps steps={steps} defaultOpen={false} />}
        {msg.supportingDataJson && <EvidencePanel supportingData={msg.supportingDataJson} />}
        {totalTokens > 0 && (
          <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
            {toolCallCount > 0 && <span>{toolCallCount} tool calls</span>}
            <span>{totalTokens} tokens</span>
          </div>
        )}
      </div>
    </div>
  );
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

  // When AgentResponse is active (streaming/completed), the refetched messages
  // may already include the assistant response for the current turn. Filter out
  // all assistant messages after the user's question to prevent duplication —
  // AgentResponse has richer data (live steps, thinking text, token usage).
  const isAgentActive = state.status === "streaming" || state.status === "completed";
  const displayMessages = ((): ConversationMessage[] => {
    if (!isAgentActive) return messages;
    const question = state.question;
    const lastQuestionIdx = messages.findLastIndex(
      (m) => m.role === "user" && m.content === question,
    );
    if (lastQuestionIdx === -1) return messages;
    return messages.filter((_, i) => i <= lastQuestionIdx);
  })();

  return (
    <div className="space-y-6">
      {displayMessages.map((msg) => (
        <div key={msg.id}>
          {msg.role === "user" ? (
            <UserMessage content={msg.content} />
          ) : (
            <HistoricalAssistantMessage msg={msg} />
          )}
        </div>
      ))}

      {conversationArtifacts.length > 0 && state.status === "idle" && (
        <ArtifactList artifacts={conversationArtifacts} />
      )}

      {state.status !== "idle" && state.status !== "error" && (
        <>
          {/* Show the user's question optimistically — it may not be in messages yet. */}
          {!messages.some((m) => m.role === "user" && m.content === state.question) && (
            <UserMessage content={state.question} />
          )}
        </>
      )}

      {state.status === "container_starting" && <ContainerStatus message={state.message} />}

      {isAgentActive && (
        <AgentResponse
          state={state}
          steps={state.steps}
          answer={state.status === "streaming" ? state.partialAnswer : state.answer}
          question={state.question}
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
