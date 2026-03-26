import { createClient } from "@connectrpc/connect";
import { useCallback, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import type {
  AgentArtifactUploaded,
  ArtifactInfo,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

import { conversationKeys } from "./use-conversations";

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
};

export type ReasoningStep = {
  kind: "reasoning";
  text: string;
  partIndex: number;
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

export const useAskQuestion = (): {
  state: AgentState;
  ask: (question: string, conversationId?: string) => Promise<void>;
  cancel: () => void;
  reset: () => void;
} => {
  const [state, setState] = useState<AgentState>({ status: "idle" });
  const abortRef = useRef<AbortController | null>(null);
  const queryClient = useQueryClient();

  const ask = useCallback(
    async (question: string, conversationId?: string) => {
      const abort = new AbortController();
      abortRef.current = abort;

      const steps: AgentStep[] = [];
      let partialAnswer = "";
      const artifacts: ArtifactInfo[] = [];
      let streamConversationId: string | undefined = conversationId;

      setState({
        status: "container_starting",
        message: "Initialising agent...",
        question,
        conversationId,
      });

      try {
        const stream = client.askQuestion({ question, conversationId }, { signal: abort.signal });

        for await (const response of stream) {
          if (abort.signal.aborted) break;

          const { event } = response;
          if (!event.case) continue;

          switch (event.case) {
            case "conversationCreated": {
              streamConversationId = event.value.conversationId;
              queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
              setState({
                status: "container_starting",
                message: "Initialising agent...",
                question,
                conversationId: streamConversationId,
              });
              break;
            }

            case "containerStatus": {
              setState({
                status: "container_starting",
                message: event.value.message,
                question,
                conversationId: streamConversationId,
              });
              break;
            }

            case "toolCallStarted": {
              const callId = event.value.callId;
              const existing = callId
                ? steps.find((s): s is ToolCallStep => s.kind === "tool" && s.callId === callId)
                : undefined;
              if (!existing) {
                steps.push({
                  kind: "tool",
                  callId: callId || crypto.randomUUID(),
                  toolName: event.value.toolName,
                  argumentsJson: event.value.argumentsJson,
                  status: "running",
                });
              }
              setState({
                status: "streaming",
                question,
                conversationId: streamConversationId,
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "toolCallCompleted": {
              const completedCallId = event.value.callId;
              const targetIdx = completedCallId
                ? steps.findIndex(
                    (s): s is ToolCallStep => s.kind === "tool" && s.callId === completedCallId,
                  )
                : steps.findLastIndex(
                    (s): s is ToolCallStep =>
                      s.kind === "tool" &&
                      s.toolName === event.value.toolName &&
                      s.status === "running",
                  );
              if (targetIdx !== -1) {
                const old = steps[targetIdx] as ToolCallStep;
                steps[targetIdx] = {
                  ...old,
                  resultSummary: event.value.resultSummary,
                  durationMs: event.value.durationMs,
                  success: event.value.success,
                  status: event.value.success ? "completed" : "error",
                };
              }
              setState({
                status: "streaming",
                question,
                conversationId: streamConversationId,
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "partialAnswer": {
              partialAnswer = event.value.text;
              setState({
                status: "streaming",
                question,
                conversationId: streamConversationId,
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "thinking": {
              // OpenCode sends cumulative text per reasoning part — the
              // part_index identifies which block is being updated.
              //
              // part_index can be recycled across agent turns (a new
              // assistant message resets the index to 0). We detect this
              // by checking whether the incoming text is a continuation
              // of the existing text (cumulative) or a fresh start (new
              // block). Search from the end so we match the most recent
              // block when duplicates exist.
              const idx = event.value.partIndex;
              const existingIdx = steps.findLastIndex(
                (s): s is ReasoningStep => s.kind === "reasoning" && s.partIndex === idx,
              );
              if (existingIdx !== -1) {
                const existing = steps[existingIdx] as ReasoningStep;
                const isContinuation =
                  event.value.text.startsWith(existing.text) ||
                  existing.text.startsWith(event.value.text);

                if (!isContinuation) {
                  // New reasoning block with a recycled partIndex — preserve old block.
                  steps.push({ kind: "reasoning", text: event.value.text, partIndex: idx });
                } else if (existingIdx === steps.length - 1) {
                  // Still the last entry — replace with new object for React.
                  steps[existingIdx] = {
                    kind: "reasoning",
                    text: event.value.text,
                    partIndex: idx,
                  };
                } else {
                  // Interleaved by tool calls — move to end.
                  steps.splice(existingIdx, 1);
                  steps.push({ kind: "reasoning", text: event.value.text, partIndex: idx });
                }
              } else {
                steps.push({ kind: "reasoning", text: event.value.text, partIndex: idx });
              }
              setState({
                status: "streaming",
                question,
                conversationId: streamConversationId,
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "artifactUploaded": {
              artifacts.push(toArtifactInfo(event.value));
              setState({
                status: "streaming",
                question,
                conversationId: streamConversationId,
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "finalAnswer": {
              const final = event.value;
              setState({
                status: "completed",
                question,
                steps: [...steps],
                answer: final.answer,
                conversationId: final.conversationId,
                supportingData: final.supportingDataJson,
                tokenUsage: {
                  promptTokens: final.promptTokens,
                  completionTokens: final.completionTokens,
                  estimatedCostUsd: final.estimatedCostUsd,
                },
                durationMs: final.durationMs,
                artifacts: [...artifacts, ...final.artifacts],
              });
              queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
              if (final.conversationId) {
                queryClient.invalidateQueries({
                  queryKey: conversationKeys.detail(final.conversationId),
                });
              }
              break;
            }

            case "error": {
              setState({
                status: "error",
                message: event.value.message,
                retryable: event.value.retryable,
              });
              break;
            }
          }
        }
      } catch (err) {
        if (!abort.signal.aborted) {
          setState({
            status: "error",
            message: err instanceof Error ? err.message : "An unexpected error occurred",
            retryable: true,
          });
        }
      }
    },
    [queryClient],
  );

  const cancel = useCallback(() => {
    abortRef.current?.abort();
    setState({ status: "idle" });
  }, []);

  const reset = useCallback(() => {
    setState({ status: "idle" });
  }, []);

  return { state, ask, cancel, reset };
};
