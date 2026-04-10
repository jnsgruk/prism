import { deriveSteps, type StreamEvent } from "@/views/ask/lib/derive-steps";
import { describe, expect, it } from "vitest";

describe("deriveSteps", () => {
  it("returns empty array for no events", () => {
    expect(deriveSteps([])).toEqual([]);
  });

  it("creates reasoning step from thinking event", () => {
    const events: StreamEvent[] = [
      {
        id: 1,
        eventType: "thinking",
        stepId: "think-0-0",
        stepSeq: 0,
        payload: { text: "I should query the database", part_index: 0 },
      },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(1);
    expect(steps[0]?.kind).toBe("reasoning");
    expect(steps[0]?.kind === "reasoning" && steps[0].text).toBe("I should query the database");
  });

  it("cumulative thinking updates replace text without changing order", () => {
    const events: StreamEvent[] = [
      {
        id: 1,
        eventType: "thinking",
        stepId: "think-0-0",
        stepSeq: 0,
        payload: { text: "I think", part_index: 0 },
      },
      {
        id: 2,
        eventType: "tool_call_started",
        stepId: "tool-abc",
        stepSeq: 1,
        payload: { tool_name: "bash", call_id: "abc", arguments_json: "{}" },
      },
      {
        id: 3,
        eventType: "thinking",
        stepId: "think-0-0",
        stepSeq: 0,
        payload: { text: "I think we should query", part_index: 0 },
      },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(2);
    // Thinking step stays first (seq=0), tool second (seq=1).
    expect(steps[0]?.kind).toBe("reasoning");
    expect(steps[0]?.kind === "reasoning" && steps[0].text).toBe("I think we should query");
    expect(steps[1]?.kind).toBe("tool");
  });

  it("tool started + completed merge into single step", () => {
    const events: StreamEvent[] = [
      {
        id: 1,
        eventType: "tool_call_started",
        stepId: "tool-abc",
        stepSeq: 0,
        payload: { tool_name: "bash", call_id: "abc", arguments_json: "{}" },
      },
      {
        id: 2,
        eventType: "tool_call_completed",
        stepId: "tool-abc",
        stepSeq: 0,
        payload: {
          tool_name: "bash",
          call_id: "abc",
          result_summary: "ok",
          duration_ms: 100,
          success: true,
        },
      },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(1);
    expect(steps[0]?.kind).toBe("tool");
    expect(steps[0]?.kind === "tool" && steps[0].status).toBe("completed");
    expect(steps[0]?.kind === "tool" && steps[0].resultSummary).toBe("ok");
  });

  it("preserves server-determined order regardless of event arrival", () => {
    // Events arrive out of step_seq order (e.g. from resume replay).
    const events: StreamEvent[] = [
      {
        id: 3,
        eventType: "tool_call_started",
        stepId: "tool-b",
        stepSeq: 2,
        payload: { tool_name: "read", call_id: "b", arguments_json: "{}" },
      },
      {
        id: 1,
        eventType: "thinking",
        stepId: "think-0-0",
        stepSeq: 0,
        payload: { text: "first", part_index: 0 },
      },
      {
        id: 2,
        eventType: "tool_call_started",
        stepId: "tool-a",
        stepSeq: 1,
        payload: { tool_name: "bash", call_id: "a", arguments_json: "{}" },
      },
    ];
    const steps = deriveSteps(events);
    expect(steps.map((s) => s.stepId)).toEqual(["think-0-0", "tool-a", "tool-b"]);
  });

  it("ignores events without stepId", () => {
    const events: StreamEvent[] = [
      {
        id: 1,
        eventType: "partial_answer",
        stepId: "",
        stepSeq: 0,
        payload: { text: "answer text" },
      },
    ];
    expect(deriveSteps(events)).toEqual([]);
  });

  it("handles recycled part_index as separate steps", () => {
    const events: StreamEvent[] = [
      {
        id: 1,
        eventType: "thinking",
        stepId: "think-0-0",
        stepSeq: 0,
        payload: { text: "first thought", part_index: 0 },
      },
      {
        id: 2,
        eventType: "tool_call_started",
        stepId: "tool-a",
        stepSeq: 1,
        payload: { tool_name: "bash", call_id: "a", arguments_json: "{}" },
      },
      {
        id: 3,
        eventType: "thinking",
        stepId: "think-0-1",
        stepSeq: 2,
        payload: { text: "second thought", part_index: 0 },
      },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(3);
    expect(steps[0]?.kind === "reasoning" && steps[0].text).toBe("first thought");
    expect(steps[2]?.kind === "reasoning" && steps[2].text).toBe("second thought");
  });
});
