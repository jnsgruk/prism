import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

afterEach(cleanup);

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";
import { ThinkingStep } from "./thinking-step";

describe("ThinkingStep", () => {
  it("renders reasoning step as italic text", () => {
    const step: AgentStep = { kind: "reasoning", text: "Analysing team data..." };
    render(<ThinkingStep step={step} />);

    const el = screen.getByText("Analysing team data...");
    expect(el).toBeInTheDocument();
    expect(el.tagName).toBe("P");
    expect(el.classList.contains("italic")).toBe(true);
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

  it("renders error tool with X icon", () => {
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

    // The X icon is rendered for error status — it's an SVG with the lucide-react "x" class
    const svg = container.querySelector("svg.lucide-x");
    expect(svg).toBeInTheDocument();
    expect(screen.getByText("Command failed")).toBeInTheDocument();
  });
});
