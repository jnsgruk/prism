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
};

export type AgentStep = ToolCallStep | ReasoningStep;

export type TokenUsage = {
  promptTokens: number;
  completionTokens: number;
  estimatedCostUsd: number;
};

export type AgentState =
  | { status: "idle" }
  | { status: "container_starting"; message: string }
  | {
      status: "streaming";
      steps: AgentStep[];
      partialAnswer: string;
      artifacts: ArtifactInfo[];
    }
  | {
      status: "completed";
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

      setState({ status: "container_starting", message: "Initialising agent..." });

      try {
        const stream = client.askQuestion({ question, conversationId }, { signal: abort.signal });

        for await (const response of stream) {
          if (abort.signal.aborted) break;

          const { event } = response;
          if (!event.case) continue;

          switch (event.case) {
            case "containerStatus": {
              setState({ status: "container_starting", message: event.value.message });
              break;
            }

            case "toolCallStarted": {
              steps.push({
                kind: "tool",
                toolName: event.value.toolName,
                argumentsJson: event.value.argumentsJson,
                status: "running",
              });
              setState({
                status: "streaming",
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "toolCallCompleted": {
              const last = steps.findLast(
                (s): s is ToolCallStep =>
                  s.kind === "tool" &&
                  s.toolName === event.value.toolName &&
                  s.status === "running",
              );
              if (last) {
                last.resultSummary = event.value.resultSummary;
                last.durationMs = event.value.durationMs;
                last.success = event.value.success;
                last.status = event.value.success ? "completed" : "error";
              }
              setState({
                status: "streaming",
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
                steps: [...steps],
                partialAnswer,
                artifacts: [...artifacts],
              });
              break;
            }

            case "thinking": {
              // Update existing reasoning step or add a new one.
              // OpenCode sends cumulative text for each reasoning part,
              // so we update the last reasoning step if it's the most
              // recent entry (i.e. no tool calls happened since).
              const lastStep = steps[steps.length - 1];
              if (lastStep?.kind === "reasoning") {
                lastStep.text = event.value.text;
              } else {
                steps.push({ kind: "reasoning", text: event.value.text });
              }
              setState({
                status: "streaming",
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
