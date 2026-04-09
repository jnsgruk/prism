import { useEffect, useMemo, useRef } from "react";
import { AlertCircle, RefreshCw, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";

import type { AgentState, AgentStep } from "@/views/ask/hooks/use-ask-question";
import type { ConversationMessage } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { AgentResponse } from "./agent-response";
import { ContainerStatus } from "./container-status";
import { EvidencePanel } from "./evidence-panel";
import { ThinkingSteps } from "./thinking-steps";
import { UserMessage } from "./user-message";
import { AnswerContent } from "./answer-content";

const parseReasoningTrace = (json?: string): AgentStep[] => {
  if (!json) return [];
  try {
    const trace = JSON.parse(json);
    return (trace.steps ?? [])
      .filter(
        (s: { kind?: string; text?: string }) =>
          // Drop empty reasoning entries left by intermediate cumulative updates.
          !(s.kind === "reasoning" && !s.text),
      )
      .map(
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
            step_id?: string;
          },
          i: number,
        ): AgentStep => {
          const stepId = s.step_id ?? undefined;
          if (s.kind === "reasoning") {
            return {
              kind: "reasoning" as const,
              text: s.text ?? "",
              partIndex: s.part_index ?? i,
              stepId,
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
            stepId,
          };
        },
      );
  } catch {
    return [];
  }
};

const HistoricalAssistantMessage = ({
  msg,
  conversationId,
}: {
  msg: ConversationMessage;
  conversationId?: string;
}): React.ReactElement => {
  const steps = parseReasoningTrace(msg.reasoningTraceJson);

  return (
    <div className="flex gap-3">
      <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-violet-100 text-violet-700 dark:bg-violet-900/30 dark:text-violet-400">
        <Sparkles className="size-3.5" />
      </div>
      <div className="min-w-0 flex-1 space-y-3 pt-0.5">
        {steps.length > 0 && <ThinkingSteps steps={steps} defaultOpen={false} />}
        <AnswerContent content={msg.content} conversationId={conversationId} />
        {msg.supportingDataJson && <EvidencePanel supportingData={msg.supportingDataJson} />}
      </div>
    </div>
  );
};

/** Inline error message rendered for `role = "error"` messages in history
 *  and for live error states. Includes a retry button when `onRetry` is provided. */
const InlineError = ({
  content,
  onRetry,
}: {
  content: string;
  onRetry?: () => void;
}): React.ReactElement => (
  <div className="flex gap-3">
    <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-destructive/10 text-destructive">
      <AlertCircle className="size-3.5" />
    </div>
    <div className="min-w-0 flex-1 space-y-2 pt-0.5">
      <p className="text-sm text-muted-foreground">{content}</p>
      {onRetry && (
        <Button variant="outline" size="sm" className="gap-1.5" onClick={onRetry}>
          <RefreshCw className="size-3.5" />
          Retry
        </Button>
      )}
    </div>
  </div>
);

/**
 * Deduplicate consecutive user messages with identical content. When a user
 * retries a failed question, the backend stores a new user message for each
 * attempt. This collapses those runs into a single message so the thread
 * stays clean.
 */
const deduplicateMessages = (msgs: ConversationMessage[]): ConversationMessage[] => {
  const result: ConversationMessage[] = [];
  for (const msg of msgs) {
    const prev = result.at(-1);
    if (prev && msg.role === "user" && prev.role === "user" && msg.content === prev.content) {
      continue; // Skip duplicate.
    }
    result.push(msg);
  }
  return result;
};

export const ConversationThread = ({
  messages,
  state,
  onRetry,
  conversationId,
  onFileClick,
  submittedFiles,
}: {
  messages: ConversationMessage[];
  state: AgentState;
  onRetry?: (question: string) => void;
  conversationId?: string;
  onFileClick?: (path: string) => void;
  submittedFiles?: string[];
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
  const displayMessages = useMemo((): ConversationMessage[] => {
    const deduped = deduplicateMessages(messages);
    if (!isAgentActive) return deduped;

    // Find the current question in messages and cut off everything after it,
    // so the server's copy of the current turn doesn't duplicate AgentResponse.
    // If the question isn't in messages yet (optimistic submit), show everything.
    const question = state.question;
    const cutoffIdx = question
      ? deduped.findLastIndex((m) => m.role === "user" && m.content === question)
      : -1;

    if (cutoffIdx === -1) return deduped;
    return deduped.filter((_, i) => i <= cutoffIdx);
  }, [messages, isAgentActive, state]);

  return (
    <div className="space-y-6">
      {displayMessages.map((msg, idx) => {
        const retryHandler = onRetry
          ? (): void => {
              const prev = displayMessages.slice(0, idx).findLast((m) => m.role === "user");
              if (prev) onRetry(prev.content);
            }
          : undefined;

        let content: React.ReactNode;
        if (msg.role === "user") {
          content = (
            <UserMessage
              content={msg.content}
              attachedFiles={msg.attachedFiles.length > 0 ? [...msg.attachedFiles] : undefined}
              mentions={msg.mentions.length > 0 ? [...msg.mentions] : undefined}
              onFileClick={onFileClick}
            />
          );
        } else if (msg.role === "error") {
          content = <InlineError content={msg.content} onRetry={retryHandler} />;
        } else {
          content = <HistoricalAssistantMessage msg={msg} conversationId={conversationId} />;
        }

        return <div key={msg.id}>{content}</div>;
      })}

      {state.status !== "idle" && state.status !== "error" && state.question && (
        <>
          {/* Show the user's question optimistically — it may not be in messages yet. */}
          {!messages.some((m) => m.role === "user" && m.content === state.question) && (
            <UserMessage
              content={state.question}
              attachedFiles={
                submittedFiles && submittedFiles.length > 0 ? submittedFiles : undefined
              }
              onFileClick={onFileClick}
            />
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
          supportingData={state.status === "completed" ? state.supportingData : undefined}
          conversationId={conversationId}
        />
      )}

      {/* Live error — rendered inline with retry. The error is also persisted
          as a role="error" message by the backend, so on reload it appears
          in the history above. */}
      {state.status === "error" && (
        <InlineError
          content={state.message}
          onRetry={
            onRetry
              ? () => {
                  // Find the question that triggered this error: either from
                  // the live state or from the last user message in history.
                  const question =
                    state.message && messages.length > 0
                      ? messages.findLast((m) => m.role === "user")?.content
                      : undefined;
                  if (question) onRetry(question);
                }
              : undefined
          }
        />
      )}

      <div ref={bottomRef} />
    </div>
  );
};
