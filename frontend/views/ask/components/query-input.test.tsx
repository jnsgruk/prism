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
  contextUsage: undefined,
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

  it("renders plus button for file attachment when onFilesAdded provided", () => {
    const { container } = renderWithProviders(
      <QueryInput {...defaultProps} onFilesAdded={vi.fn()} />,
    );
    const plusIcon = container.querySelector("svg.lucide-plus");
    expect(plusIcon).toBeInTheDocument();
  });

  it("submit enabled with files but no text", () => {
    renderWithProviders(
      <QueryInput
        {...defaultProps}
        attachedFiles={[{ name: "file.pdf", size: 1000 }]}
        onFilesAdded={vi.fn()}
      />,
    );
    const buttons = screen.getAllByRole("button");
    const submitButton = buttons.find((b) => b.querySelector("svg.lucide-arrow-up"));
    expect(submitButton).not.toBeDisabled();
  });

  it("shows file chips when files attached", () => {
    renderWithProviders(
      <QueryInput
        {...defaultProps}
        attachedFiles={[{ name: "report.pdf", size: 42000 }]}
        onFilesAdded={vi.fn()}
      />,
    );
    expect(screen.getByText("report.pdf")).toBeInTheDocument();
  });

  it("calls onFileRemoved when chip X clicked", () => {
    const onFileRemoved = vi.fn();
    renderWithProviders(
      <QueryInput
        {...defaultProps}
        attachedFiles={[
          { name: "first.txt", size: 100 },
          { name: "second.txt", size: 200 },
        ]}
        onFilesAdded={vi.fn()}
        onFileRemoved={onFileRemoved}
      />,
    );
    fireEvent.click(screen.getByLabelText("Remove second.txt"));
    expect(onFileRemoved).toHaveBeenCalledWith(1);
  });

  it("shows drag-active styling on dragover", () => {
    renderWithProviders(<QueryInput {...defaultProps} onFilesAdded={vi.fn()} />);
    const textarea = screen.getByPlaceholderText("Ask a question about your engineering data...");
    // The drop zone is the parent container of the textarea
    const dropZone = textarea.closest("[class*='rounded-lg']")!;
    fireEvent.dragOver(dropZone);
    expect(dropZone.className).toContain("ring-primary");
  });

  it("does not intercept text-only paste", () => {
    const onFilesAdded = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onFilesAdded={onFilesAdded} />);
    const textarea = screen.getByPlaceholderText("Ask a question about your engineering data...");
    fireEvent.paste(textarea, {
      clipboardData: { files: [] },
    });
    expect(onFilesAdded).not.toHaveBeenCalled();
  });
});
