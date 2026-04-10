import { createClient } from "@connectrpc/connect";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ReasoningService, type MentionType } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

/** Plain mention shape accepted by the ask hook (avoids protobuf-es Message wrapper). */
export type MentionInit = {
  id: string;
  name: string;
  type: MentionType;
};

import { conversationKeys } from "@/lib/hooks/use-conversations";
import { deriveSteps, type StreamEvent } from "@/views/ask/lib/derive-steps";

const client = createClient(ReasoningService, transport);

export type ToolCallStep = {
  kind: "tool";
  callId: string;
  toolName: string;
  argumentsJson: string;
  resultSummary?: string;
  durationMs?: number;
  success?: boolean;
  status: "running" | "completed" | "error";
  stepId?: string;
};

export type ReasoningStep = {
  kind: "reasoning";
  text: string;
  partIndex: number;
  stepId?: string;
};

export type AgentStep = ToolCallStep | ReasoningStep;

export type TokenUsage = {
  promptTokens: number;
  completionTokens: number;
  estimatedCostUsd: number;
};

export type ContextUsage = {
  inputTokens: number;
  outputTokens: number;
  contextWindow: number;
};

export type AgentState =
  | { status: "idle" }
  | { status: "container_starting"; message: string; question: string; conversationId?: string }
  | {
      status: "streaming";
      question: string;
      conversationId?: string;
      steps: AgentStep[];
      partialAnswer: string;
      contextUsage?: ContextUsage;
    }
  | {
      status: "completed";
      question: string;
      steps: AgentStep[];
      answer: string;
      conversationId: string;
      supportingData: string;
      tokenUsage: TokenUsage;
      durationMs: number;
      contextUsage?: ContextUsage;
    }
  | { status: "cancelling"; conversationId?: string; question?: string }
  | { status: "error"; message: string; retryable: boolean };

type StreamMeta =
  | { status: "idle" }
  | { status: "container_starting"; message: string; question: string; conversationId?: string }
  | {
      status: "streaming";
      question: string;
      conversationId?: string;
      partialAnswer: string;
      contextUsage?: ContextUsage;
    }
  | {
      status: "completed";
      question: string;
      answer: string;
      conversationId: string;
      supportingData: string;
      tokenUsage: TokenUsage;
      durationMs: number;
      contextUsage?: ContextUsage;
    }
  | { status: "cancelling"; conversationId?: string; question?: string }
  | { status: "error"; message: string; retryable: boolean };

export const useAskQuestion = (): {
  state: AgentState;
  ask: (
    question: string,
    conversationId?: string,
    modelOverride?: string,
    attachedFiles?: string[],
    mentions?: MentionInit[],
  ) => Promise<void>;
  cancel: () => Promise<void>;
  reset: () => void;
  resume: (conversationId: string, question?: string) => Promise<void>;
} => {
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [meta, setMeta] = useState<StreamMeta>({ status: "idle" });
  const abortRef = useRef<AbortController | null>(null);
  const queryClient = useQueryClient();
  const nextEventId = useRef(0);

  // Derive steps from events -- pure, deterministic, memoized.
  const steps = useMemo(() => deriveSteps(events), [events]);

  // Helper to append a step event from a proto response.
  const appendStepEvent = useCallback(
    (eventCase: string, value: { stepId?: string; stepSeq?: number; [key: string]: unknown }): void => {
      let stepId = "";
      let stepSeq = 0;
      let eventType = "";
      let payload: Record<string, unknown> = {};

      switch (eventCase) {
        case "thinking":
          stepId = (value.stepId as string) ?? "";
          stepSeq = (value.stepSeq as number) ?? 0;
          eventType = "thinking";
          payload = { text: value.text, part_index: value.partIndex };
          break;
        case "toolCallStarted":
          stepId = (value.stepId as string) ?? "";
          stepSeq = (value.stepSeq as number) ?? 0;
          eventType = "tool_call_started";
          payload = {
            tool_name: value.toolName,
            arguments_json: value.argumentsJson,
            call_id: value.callId,
          };
          break;
        case "toolCallCompleted":
          stepId = (value.stepId as string) ?? "";
          stepSeq = (value.stepSeq as number) ?? 0;
          eventType = "tool_call_completed";
          payload = {
            tool_name: value.toolName,
            result_summary: value.resultSummary,
            duration_ms: value.durationMs,
            success: value.success,
            call_id: value.callId,
          };
          break;
        default:
          return;
      }

      if (!stepId) return;

      const id = nextEventId.current++;
      setEvents((prev) => [...prev, { id, eventType, stepId, stepSeq, payload }]);
    },
    [],
  );

  /** Process a stream of AskQuestion/ResumeStream responses. */
  const processStream = useCallback(
    async (
      stream: AsyncIterable<{ event: { case?: string; value?: Record<string, unknown> } }>,
      abort: AbortController,
      question: string,
      initialConversationId?: string,
    ) => {
      let partialAnswer = "";
      let streamConversationId: string | undefined = initialConversationId;

      for await (const response of stream) {
        if (abort.signal.aborted) break;

        const { event } = response;
        if (!event.case) continue;

        switch (event.case) {
          case "conversationCreated": {
            const v = event.value as { conversationId: string };
            streamConversationId = v.conversationId;
            queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
            setMeta((prev) =>
              prev.status === "container_starting" ? { ...prev, conversationId: streamConversationId } : prev,
            );
            break;
          }

          case "containerStatus": {
            const v = event.value as { message: string };
            setMeta({
              status: "container_starting",
              message: v.message,
              question,
              conversationId: streamConversationId,
            });
            break;
          }

          case "partialAnswer": {
            const v = event.value as { text: string };
            partialAnswer = v.text;
            setMeta({
              status: "streaming",
              question,
              conversationId: streamConversationId,
              partialAnswer,
            });
            break;
          }

          case "finalAnswer": {
            const v = event.value as {
              answer: string;
              conversationId: string;
              supportingDataJson: string;
              promptTokens: number;
              completionTokens: number;
              estimatedCostUsd: number;
              durationMs: number;
            };
            setMeta({
              status: "completed",
              question,
              answer: v.answer,
              conversationId: v.conversationId,
              supportingData: v.supportingDataJson,
              tokenUsage: {
                promptTokens: v.promptTokens,
                completionTokens: v.completionTokens,
                estimatedCostUsd: v.estimatedCostUsd,
              },
              durationMs: v.durationMs,
            });
            queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
            if (v.conversationId) {
              queryClient.invalidateQueries({
                queryKey: conversationKeys.detail(v.conversationId),
              });
            }
            break;
          }

          case "tokenUsage": {
            const v = event.value as {
              inputTokens: bigint;
              outputTokens: bigint;
              contextWindow: bigint;
            };
            const usage: ContextUsage = {
              inputTokens: Number(v.inputTokens),
              outputTokens: Number(v.outputTokens),
              contextWindow: Number(v.contextWindow),
            };
            setMeta((prev) => (prev.status === "streaming" ? { ...prev, contextUsage: usage } : prev));
            break;
          }

          case "error": {
            const v = event.value as { message: string; retryable: boolean };
            setMeta({ status: "error", message: v.message, retryable: v.retryable });
            break;
          }

          case "thinking":
          case "toolCallStarted":
          case "toolCallCompleted": {
            // Transition from container_starting to streaming on first step event.
            setMeta((prev) =>
              prev.status === "container_starting"
                ? {
                    status: "streaming",
                    question,
                    conversationId: streamConversationId,
                    partialAnswer,
                  }
                : prev,
            );
            appendStepEvent(event.case, event.value as Record<string, unknown>);
            break;
          }
        }
      }
    },
    [queryClient, appendStepEvent],
  );

  const ask = useCallback(
    async (
      question: string,
      conversationId?: string,
      modelOverride?: string,
      attachedFiles?: string[],
      mentions?: MentionInit[],
    ) => {
      abortRef.current?.abort();
      const abort = new AbortController();
      abortRef.current = abort;
      setEvents([]);
      nextEventId.current = 0;

      setMeta({
        status: "container_starting",
        message: "Initialising agent...",
        question,
        conversationId,
      });

      try {
        // Parse image model prefix: "image:provider/model" → imageModel field.
        let effectiveModelOverride = modelOverride;
        let imageModel: string | undefined;
        if (modelOverride?.startsWith("image:")) {
          imageModel = modelOverride.slice(6);
          effectiveModelOverride = undefined; // Use default chat model for reasoning.
        }

        const stream = client.askQuestion(
          {
            question,
            conversationId,
            modelOverride: effectiveModelOverride,
            imageModel,
            attachedFiles: attachedFiles ?? [],
            mentions: mentions ?? [],
          },
          { signal: abort.signal },
        );
        await processStream(stream, abort, question, conversationId);
      } catch (err) {
        if (!abort.signal.aborted) {
          setMeta({
            status: "error",
            message: err instanceof Error ? err.message : "An unexpected error occurred",
            retryable: true,
          });
        }
      }
      // If the stream ended without a terminal event (finalAnswer or error),
      // reset to idle so the stored conversation data is displayed.
      setMeta((prev) =>
        prev.status === "streaming" || prev.status === "container_starting" ? { status: "idle" } : prev,
      );
    },
    [processStream],
  );

  const resume = useCallback(
    async (conversationId: string, question?: string) => {
      const q = question ?? "";
      abortRef.current?.abort();
      const abort = new AbortController();
      abortRef.current = abort;
      setEvents([]);
      nextEventId.current = 0;

      setMeta({
        status: "streaming",
        question: q,
        conversationId,
        partialAnswer: "",
      });

      try {
        const stream = client.resumeStream({ conversationId, lastEventId: BigInt(0) }, { signal: abort.signal });
        await processStream(stream, abort, q, conversationId);
      } catch (err) {
        if (!abort.signal.aborted) {
          setMeta({
            status: "error",
            message: err instanceof Error ? err.message : "Connection lost",
            retryable: true,
          });
        }
      }
      // If the stream ended without a terminal event (e.g., the query was
      // already completed when we connected), reset to idle so the stored
      // conversation data is displayed instead of a stuck spinner.
      setMeta((prev) =>
        prev.status === "streaming" || prev.status === "container_starting" ? { status: "idle" } : prev,
      );
      // Refresh conversation data so any response completed while we were
      // away is shown from the database rather than stale cache.
      queryClient.invalidateQueries({ queryKey: conversationKeys.detail(conversationId) });
    },
    [processStream, queryClient],
  );

  // Track the current conversation ID so the stable `cancel` callback can
  // reference it without closing over changing state.
  const conversationIdRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (
      meta.status === "streaming" ||
      meta.status === "container_starting" ||
      meta.status === "completed" ||
      meta.status === "cancelling"
    ) {
      conversationIdRef.current = meta.conversationId;
    }
  }, [meta]);

  const cancel = useCallback(async () => {
    abortRef.current?.abort();
    setEvents([]);
    const convId = conversationIdRef.current;
    if (convId) {
      setMeta({ status: "cancelling", conversationId: convId });
      try {
        await client.cancelQuery({ conversationId: convId });
      } catch {
        // If the RPC fails, the reaper will eventually clean up the pod.
      }
    }
    setMeta({ status: "idle" });
    queryClient.invalidateQueries({ queryKey: conversationKeys.all });
  }, [queryClient]);

  const reset = useCallback(() => {
    abortRef.current?.abort();
    setMeta({ status: "idle" });
    setEvents([]);
  }, []);

  // Build full AgentState by combining meta + derived steps.
  const state: AgentState = useMemo(() => {
    if (meta.status === "streaming") {
      return { ...meta, steps };
    }
    if (meta.status === "completed") {
      return { ...meta, steps };
    }
    return meta;
  }, [meta, steps]);

  return { state, ask, cancel, reset, resume };
};
