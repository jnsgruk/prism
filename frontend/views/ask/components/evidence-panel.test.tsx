import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { EvidencePanel } from "./evidence-panel";

afterEach(cleanup);

describe("EvidencePanel", () => {
  it("renders Evidence & Reasoning trigger text", () => {
    render(<EvidencePanel />);

    expect(screen.getByText("Evidence & Reasoning")).toBeInTheDocument();
  });

  it("shows supporting data when provided and expanded", async () => {
    const supportingData = JSON.stringify({ teams: [{ name: "Alpha", velocity: 42 }] });
    render(<EvidencePanel supportingData={supportingData} />);

    // Expand
    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    await waitFor(() => {
      expect(screen.getByText("Supporting data")).toBeInTheDocument();
    });
    expect(screen.getByText(/Alpha/)).toBeInTheDocument();
  });

  it("does not show supporting data section when supportingData is empty object", async () => {
    render(<EvidencePanel supportingData="{}" />);

    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    // The collapsible opens but no supporting data section
    await waitFor(() => {
      expect(screen.queryByText("Supporting data")).not.toBeInTheDocument();
    });
  });

  it("does not show supporting data section when supportingData is null string", async () => {
    render(<EvidencePanel supportingData="null" />);

    fireEvent.click(screen.getByText("Evidence & Reasoning"));

    await waitFor(() => {
      expect(screen.queryByText("Supporting data")).not.toBeInTheDocument();
    });
  });
});
