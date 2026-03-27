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
export const deriveSteps = (events: StreamEvent[]): AgentStep[] => {
  const stepMap = new Map<string, { seq: number; step: AgentStep }>();

  for (const event of events) {
    if (!event.stepId) continue;

    switch (event.eventType) {
      case "thinking": {
        const existing = stepMap.get(event.stepId);
        const text = (event.payload.text as string) ?? "";
        const partIndex = (event.payload.part_index as number) ?? 0;

        if (existing && existing.step.kind === "reasoning") {
          // Cumulative update -- replace text, keep position.
          stepMap.set(event.stepId, {
            seq: existing.seq,
            step: { kind: "reasoning", text, partIndex, stepId: event.stepId } as ReasoningStep,
          });
        } else {
          stepMap.set(event.stepId, {
            seq: event.stepSeq,
            step: { kind: "reasoning", text, partIndex, stepId: event.stepId } as ReasoningStep,
          });
        }
        break;
      }

      case "tool_call_started": {
        stepMap.set(event.stepId, {
          seq: event.stepSeq,
          step: {
            kind: "tool",
            callId: (event.payload.call_id as string) ?? "",
            toolName: (event.payload.tool_name as string) ?? "",
            argumentsJson: (event.payload.arguments_json as string) ?? "{}",
            status: "running",
            stepId: event.stepId,
          } as ToolCallStep,
        });
        break;
      }

      case "tool_call_completed": {
        const existing = stepMap.get(event.stepId);
        if (existing && existing.step.kind === "tool") {
          stepMap.set(event.stepId, {
            seq: existing.seq,
            step: {
              ...existing.step,
              resultSummary: event.payload.result_summary as string,
              durationMs: event.payload.duration_ms as number,
              success: event.payload.success as boolean,
              status: (event.payload.success as boolean) ? "completed" : "error",
            },
          });
        }
        break;
      }
    }
  }

  return [...stepMap.values()].toSorted((a, b) => a.seq - b.seq).map(({ step }) => step);
};
