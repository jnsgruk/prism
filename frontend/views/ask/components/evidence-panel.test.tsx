import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";
import { EvidencePanel } from "./evidence-panel";

afterEach(cleanup);

const sampleSteps: AgentStep[] = [
  { kind: "reasoning", text: "Thinking about team data..." },
  {
    kind: "tool",
    toolName: "mcp_prism_list_teams",
    argumentsJson: "{}",
    status: "completed",
    durationMs: 200,
    success: true,
  },
];

describe("EvidencePanel", () => {
  it("renders Evidence & Reasoning trigger text", () => {
    render(<EvidencePanel steps={sampleSteps} />);

    expect(screen.getByText("Evidence & Reasoning")).toBeInTheDocument();
  });

  it("shows thinking steps when expanded", async () => {
    render(<EvidencePanel steps={sampleSteps} />);

    // Click to expand
    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    await waitFor(() => {
      expect(screen.getByText("Thinking about team data...")).toBeInTheDocument();
    });
  });

  it("shows supporting data when provided and expanded", async () => {
    const supportingData = JSON.stringify({ teams: [{ name: "Alpha", velocity: 42 }] });
    render(<EvidencePanel steps={sampleSteps} supportingData={supportingData} />);

    // Expand
    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    await waitFor(() => {
      expect(screen.getByText("Supporting data")).toBeInTheDocument();
    });
    expect(screen.getByText(/Alpha/)).toBeInTheDocument();
  });

  it("does not show supporting data section when supportingData is empty object", async () => {
    render(<EvidencePanel steps={sampleSteps} supportingData="{}" />);

    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    // Wait for the collapsible to open by checking for the agent activity section
    await waitFor(() => {
      expect(screen.getByText("Thinking about team data...")).toBeInTheDocument();
    });

    expect(screen.queryByText("Supporting data")).not.toBeInTheDocument();
  });

  it("does not show supporting data section when supportingData is null string", async () => {
    render(<EvidencePanel steps={sampleSteps} supportingData="null" />);

    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    await waitFor(() => {
      expect(screen.getByText("Thinking about team data...")).toBeInTheDocument();
    });

    expect(screen.queryByText("Supporting data")).not.toBeInTheDocument();
  });
});
