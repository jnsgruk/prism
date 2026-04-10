import type { AgentStep, ReasoningStep, ToolCallStep } from "@/views/ask/hooks/use-ask-question";

/**
 * Raw event from the server stream, carrying server-assigned identity.
 */
export type StreamEvent = {
  /** DB auto-increment id (arrival order, used as cursor). */
  id: number;
  eventType: string;
  stepId: string;
  stepSeq: number;
  payload: Record<string, unknown>;
};

/**
 * Derive an ordered, deduplicated step list from a sequence of stream events.
 *
 * This is a pure function -- same events in, same steps out.
 * Display order is determined solely by server-assigned `stepSeq`.
 * Thinking updates (cumulative) replace text but never change position.
 * Tool completions update an existing started step in-place.
 */
const str = (v: unknown): string => (typeof v === "string" ? v : "");
const num = (v: unknown): number => (typeof v === "number" ? v : 0);
const bool = (v: unknown): boolean => (typeof v === "boolean" ? v : false);

export const deriveSteps = (events: StreamEvent[]): AgentStep[] => {
  const stepMap = new Map<string, { seq: number; step: AgentStep }>();

  for (const event of events) {
    if (!event.stepId) continue;

    switch (event.eventType) {
      case "thinking": {
        const existing = stepMap.get(event.stepId);
        const text = str(event.payload.text);
        const partIndex = num(event.payload.part_index);
        const reasoningStep: ReasoningStep = { kind: "reasoning", text, partIndex, stepId: event.stepId };

        if (existing && existing.step.kind === "reasoning") {
          // Cumulative update -- replace text, keep position.
          stepMap.set(event.stepId, { seq: existing.seq, step: reasoningStep });
        } else {
          stepMap.set(event.stepId, { seq: event.stepSeq, step: reasoningStep });
        }
        break;
      }

      case "tool_call_started": {
        const toolStep: ToolCallStep = {
          kind: "tool",
          callId: str(event.payload.call_id),
          toolName: str(event.payload.tool_name),
          argumentsJson: str(event.payload.arguments_json) || "{}",
          status: "running",
          stepId: event.stepId,
        };
        stepMap.set(event.stepId, { seq: event.stepSeq, step: toolStep });
        break;
      }

      case "tool_call_completed": {
        const existing = stepMap.get(event.stepId);
        if (existing && existing.step.kind === "tool") {
          const success = bool(event.payload.success);
          stepMap.set(event.stepId, {
            seq: existing.seq,
            step: {
              ...existing.step,
              resultSummary: str(event.payload.result_summary),
              durationMs: num(event.payload.duration_ms),
              success,
              status: success ? "completed" : "error",
            },
          });
        }
        break;
      }
    }
  }

  return [...stepMap.values()].toSorted((a, b) => a.seq - b.seq).map(({ step }) => step);
};
