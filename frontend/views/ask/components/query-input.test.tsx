import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { QueryInput } from "./query-input";

afterEach(cleanup);

describe("QueryInput", () => {
  it("renders textarea with placeholder", () => {
    render(<QueryInput onSubmit={vi.fn()} onCancel={vi.fn()} isStreaming={false} />);

    expect(
      screen.getByPlaceholderText("Ask a question about your engineering data..."),
    ).toBeInTheDocument();
  });

  it("submit button is disabled when textarea is empty", () => {
    render(<QueryInput onSubmit={vi.fn()} onCancel={vi.fn()} isStreaming={false} />);

    const button = screen.getByRole("button");
    expect(button).toBeDisabled();
  });

  it("calls onSubmit with trimmed text when submit button clicked", () => {
    const onSubmit = vi.fn();
    render(<QueryInput onSubmit={onSubmit} onCancel={vi.fn()} isStreaming={false} />);

    const textarea = screen.getByPlaceholderText("Ask a question about your engineering data...");
    fireEvent.change(textarea, { target: { value: "  What is the team velocity?  " } });

    const button = screen.getByRole("button");
    fireEvent.click(button);

    expect(onSubmit).toHaveBeenCalledWith("What is the team velocity?");
  });

  it("calls onSubmit on Enter key (not Shift+Enter)", () => {
    const onSubmit = vi.fn();
    render(<QueryInput onSubmit={onSubmit} onCancel={vi.fn()} isStreaming={false} />);

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
    const { container } = render(
      <QueryInput onSubmit={vi.fn()} onCancel={vi.fn()} isStreaming={true} />,
    );

    const squareIcon = container.querySelector("svg.lucide-square");
    expect(squareIcon).toBeInTheDocument();
  });

  it("calls onCancel when stop button clicked", () => {
    const onCancel = vi.fn();
    render(<QueryInput onSubmit={vi.fn()} onCancel={onCancel} isStreaming={true} />);

    const button = screen.getByRole("button");
    fireEvent.click(button);

    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("clears textarea after submit", () => {
    const onSubmit = vi.fn();
    render(<QueryInput onSubmit={onSubmit} onCancel={vi.fn()} isStreaming={false} />);

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
