import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";

afterEach(cleanup);

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";

import { ThinkingStep } from "./thinking-step";

describe("ThinkingStep", () => {
  it("renders reasoning step with markdown formatting", () => {
    const step: AgentStep = { kind: "reasoning", text: "Analysing team data...", partIndex: 0 };
    render(<ThinkingStep step={step} />);

    const el = screen.getByText("Analysing team data...");
    expect(el).toBeInTheDocument();
    // Rendered inside a prose div with markdown (p tag inside div)
    expect(el.tagName).toBe("P");
    expect(el.closest(".prose")).toBeTruthy();
  });

  it("renders MCP tool call with Database icon", () => {
    const step: AgentStep = {
      kind: "tool",
      callId: "call-1",
      toolName: "mcp_prism_list_teams",
      argumentsJson: "{}",
      status: "running",
    };
    render(<ThinkingStep step={step} />);

    // The tool label replaces underscores with spaces and strips the prism_ prefix
    expect(screen.getByText("mcp prism list teams")).toBeInTheDocument();
  });

  it("renders bash tool call with command from argumentsJson", () => {
    const step: AgentStep = {
      kind: "tool",
      callId: "call-2",
      toolName: "bash",
      argumentsJson: JSON.stringify({ command: "ls -la /tmp" }),
      status: "running",
    };
    render(<ThinkingStep step={step} />);

    expect(screen.getByText("ls -la /tmp")).toBeInTheDocument();
  });

  it("renders completed tool with duration in ms", () => {
    const step: AgentStep = {
      kind: "tool",
      callId: "call-3",
      toolName: "grep",
      argumentsJson: "{}",
      status: "completed",
      durationMs: 450,
      success: true,
    };
    render(<ThinkingStep step={step} />);

    expect(screen.getByText("450ms")).toBeInTheDocument();
  });

  it("renders completed tool with duration in seconds", () => {
    const step: AgentStep = {
      kind: "tool",
      callId: "call-4",
      toolName: "grep",
      argumentsJson: "{}",
      status: "completed",
      durationMs: 2500,
      success: true,
    };
    render(<ThinkingStep step={step} />);

    expect(screen.getByText("2.5s")).toBeInTheDocument();
  });

  it("renders error tool with X icon and expandable result", () => {
    const step: AgentStep = {
      kind: "tool",
      callId: "call-5",
      toolName: "bash",
      argumentsJson: JSON.stringify({ command: "exit 1" }),
      status: "error",
      durationMs: 100,
      success: false,
      resultSummary: "Command failed",
    };
    const { container } = render(<ThinkingStep step={step} />);

    // The X icon is rendered for error status
    const svg = container.querySelector("svg.lucide-x");
    expect(svg).toBeInTheDocument();
    // Result is collapsed by default — expand chevron is present
    const chevron = container.querySelector("svg.lucide-chevron-right");
    expect(chevron).toBeInTheDocument();
    // Result not visible until expanded
    expect(screen.queryByText("Command failed")).not.toBeInTheDocument();
  });
});
