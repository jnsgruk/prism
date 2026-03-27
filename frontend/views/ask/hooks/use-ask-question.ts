import { createClient } from "@connectrpc/connect";
import { useCallback, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import type {
  AgentArtifactUploaded,
  ArtifactInfo,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

import { conversationKeys } from "./use-conversations";
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

export type AgentState =
  | { status: "idle" }
  | { status: "container_starting"; message: string; question: string; conversationId?: string }
  | {
      status: "streaming";
      question: string;
      conversationId?: string;
      steps: AgentStep[];
      partialAnswer: string;
      artifacts: ArtifactInfo[];
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
      artifacts: ArtifactInfo[];
    }
  | { status: "error"; message: string; retryable: boolean };

const toArtifactInfo = (a: AgentArtifactUploaded): ArtifactInfo =>
  ({
    id: a.artifactId,
    displayName: a.displayName,
    contentType: a.contentType,
    sizeBytes: a.sizeBytes,
  }) as ArtifactInfo;

type StreamMeta =
  | { status: "idle" }
  | { status: "container_starting"; message: string; question: string; conversationId?: string }
  | {
      status: "streaming";
      question: string;
      conversationId?: string;
      partialAnswer: string;
      artifacts: ArtifactInfo[];
    }
  | {
      status: "completed";
      question: string;
      answer: string;
      conversationId: string;
      supportingData: string;
      tokenUsage: TokenUsage;
      durationMs: number;
      artifacts: ArtifactInfo[];
    }
  | { status: "error"; message: string; retryable: boolean };

export const useAskQuestion = (): {
  state: AgentState;
  ask: (question: string, conversationId?: string) => Promise<void>;
  cancel: () => void;
  reset: () => void;
  resume: (conversationId: string) => Promise<void>;
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
    (
      eventCase: string,
      value: { stepId?: string; stepSeq?: number; [key: string]: unknown },
    ): void => {
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
      const artifacts: ArtifactInfo[] = [];
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
              prev.status === "container_starting"
                ? { ...prev, conversationId: streamConversationId }
                : prev,
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
              artifacts: [...artifacts],
            });
            break;
          }

          case "artifactUploaded": {
            const v = event.value as AgentArtifactUploaded;
            artifacts.push(toArtifactInfo(v));
            setMeta((prev) =>
              prev.status === "streaming" ? { ...prev, artifacts: [...artifacts] } : prev,
            );
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
              artifacts: ArtifactInfo[];
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
              artifacts: [...artifacts, ...v.artifacts],
            });
            queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
            if (v.conversationId) {
              queryClient.invalidateQueries({
                queryKey: conversationKeys.detail(v.conversationId),
              });
            }
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
                    artifacts: [...artifacts],
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
    async (question: string, conversationId?: string) => {
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
        const stream = client.askQuestion({ question, conversationId }, { signal: abort.signal });
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
    },
    [processStream],
  );

  const resume = useCallback(
    async (conversationId: string) => {
      const abort = new AbortController();
      abortRef.current = abort;
      setEvents([]);
      nextEventId.current = 0;

      setMeta({
        status: "streaming",
        question: "",
        conversationId,
        partialAnswer: "",
        artifacts: [],
      });

      try {
        const stream = client.resumeStream(
          { conversationId, lastEventId: BigInt(0) },
          { signal: abort.signal },
        );
        await processStream(stream, abort, "", conversationId);
      } catch (err) {
        if (!abort.signal.aborted) {
          setMeta({
            status: "error",
            message: err instanceof Error ? err.message : "Connection lost",
            retryable: true,
          });
        }
      }
    },
    [processStream],
  );

  const cancel = useCallback(() => {
    abortRef.current?.abort();
    setMeta({ status: "idle" });
    setEvents([]);
  }, []);

  const reset = useCallback(() => {
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
