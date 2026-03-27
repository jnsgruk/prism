import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { renderWithProviders, setupCleanup } from "@ps/test-utils";

import { QueryInput } from "./query-input";

const defaultProps = {
  onSubmit: vi.fn(),
  onCancel: vi.fn(),
  isStreaming: false,
  selectedModel: undefined,
  onModelChange: vi.fn(),
};

setupCleanup();

describe("QueryInput", () => {
  it("renders textarea with placeholder", () => {
    renderWithProviders(<QueryInput {...defaultProps} />);

    expect(
      screen.getByPlaceholderText("Ask a question about your engineering data..."),
    ).toBeInTheDocument();
  });

  it("submit button is disabled when textarea is empty", () => {
    renderWithProviders(<QueryInput {...defaultProps} />);

    const buttons = screen.getAllByRole("button");
    const submitButton = buttons.find((b) => b.querySelector("svg.lucide-arrow-up"));
    expect(submitButton).toBeDisabled();
  });

  it("calls onSubmit with trimmed text when submit button clicked", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const textarea = screen.getByPlaceholderText("Ask a question about your engineering data...");
    fireEvent.change(textarea, { target: { value: "  What is the team velocity?  " } });

    const buttons = screen.getAllByRole("button");
    const submitButton = buttons.find((b) => b.querySelector("svg.lucide-arrow-up"))!;
    fireEvent.click(submitButton);

    expect(onSubmit).toHaveBeenCalledWith("What is the team velocity?");
  });

  it("calls onSubmit on Enter key (not Shift+Enter)", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const textarea = screen.getByPlaceholderText("Ask a question about your engineering data...");
    fireEvent.change(textarea, { target: { value: "Test question" } });

    // Shift+Enter should not submit
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();

    // Enter without Shift should submit
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: false });
    expect(onSubmit).toHaveBeenCalledWith("Test question");
  });

  it("shows stop button when isStreaming is true", () => {
    const { container } = renderWithProviders(<QueryInput {...defaultProps} isStreaming={true} />);

    const squareIcon = container.querySelector("svg.lucide-square");
    expect(squareIcon).toBeInTheDocument();
  });

  it("calls onCancel when stop button clicked", () => {
    const onCancel = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onCancel={onCancel} isStreaming={true} />);

    const buttons = screen.getAllByRole("button");
    const stopButton = buttons.find((b) => b.querySelector("svg.lucide-square"))!;
    fireEvent.click(stopButton);

    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("clears textarea after submit", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const textarea = screen.getByPlaceholderText(
      "Ask a question about your engineering data...",
    ) as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "Hello" } });
    expect(textarea.value).toBe("Hello");

    // Submit via Enter
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: false });

    expect(textarea.value).toBe("");
  });
});
