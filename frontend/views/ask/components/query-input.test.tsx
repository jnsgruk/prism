import { createPillElement } from "@/views/ask/hooks/use-mention-picker";
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

/** Helper to get the contentEditable editor element. */
const getEditor = (): HTMLElement => screen.getByRole("textbox");

setupCleanup();

describe("QueryInput", () => {
  it("renders editor with placeholder", () => {
    renderWithProviders(<QueryInput {...defaultProps} />);

    const editor = getEditor();
    expect(editor).toBeInTheDocument();
    expect(editor.dataset.placeholder).toBe("Ask a question about your engineering data...");
  });

  it("submit button is disabled when editor is empty", () => {
    renderWithProviders(<QueryInput {...defaultProps} />);

    const buttons = screen.getAllByRole("button");
    const submitButton = buttons.find((b) => b.querySelector("svg.lucide-arrow-up"));
    expect(submitButton).toBeDisabled();
  });

  it("calls onSubmit with trimmed text when submit button clicked", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const editor = getEditor();
    editor.textContent = "  What is the team velocity?  ";
    fireEvent.input(editor);

    const buttons = screen.getAllByRole("button");
    const submitButton = buttons.find((b) => b.querySelector("svg.lucide-arrow-up"))!;
    fireEvent.click(submitButton);

    expect(onSubmit).toHaveBeenCalledWith("What is the team velocity?", undefined);
  });

  it("calls onSubmit on Enter key (not Shift+Enter)", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const editor = getEditor();
    editor.textContent = "Test question";
    fireEvent.input(editor);

    // Shift+Enter should not submit
    fireEvent.keyDown(editor, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();

    // Enter without Shift should submit
    fireEvent.keyDown(editor, { key: "Enter", shiftKey: false });
    expect(onSubmit).toHaveBeenCalledWith("Test question", undefined);
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

  it("clears editor after submit", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const editor = getEditor();
    editor.textContent = "Hello";
    fireEvent.input(editor);

    // Submit via Enter
    fireEvent.keyDown(editor, { key: "Enter", shiftKey: false });

    expect(editor.innerHTML).toBe("");
  });

  it("renders plus button for file attachment when onFilesAdded provided", () => {
    const { container } = renderWithProviders(<QueryInput {...defaultProps} onFilesAdded={vi.fn()} />);
    const plusIcon = container.querySelector("svg.lucide-plus");
    expect(plusIcon).toBeInTheDocument();
  });

  it("submit enabled with files but no text", () => {
    renderWithProviders(
      <QueryInput {...defaultProps} attachedFiles={[{ name: "file.pdf", size: 1000 }]} onFilesAdded={vi.fn()} />,
    );
    const buttons = screen.getAllByRole("button");
    const submitButton = buttons.find((b) => b.querySelector("svg.lucide-arrow-up"));
    expect(submitButton).not.toBeDisabled();
  });

  it("shows file chips when files attached", () => {
    renderWithProviders(
      <QueryInput {...defaultProps} attachedFiles={[{ name: "report.pdf", size: 42000 }]} onFilesAdded={vi.fn()} />,
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
    const editor = getEditor();
    const dropZone = editor.closest("[class*='rounded-lg']")!;
    fireEvent.dragOver(dropZone);
    expect(dropZone.className).toContain("ring-primary");
  });

  it("does not intercept text-only paste", () => {
    const onFilesAdded = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onFilesAdded={onFilesAdded} />);
    const editor = getEditor();
    fireEvent.paste(editor, {
      clipboardData: { files: [], getData: () => "plain text" },
    });
    expect(onFilesAdded).not.toHaveBeenCalled();
  });

  it("calls onSubmit with mention items when pills present", () => {
    const onSubmit = vi.fn();
    renderWithProviders(<QueryInput {...defaultProps} onSubmit={onSubmit} />);

    const editor = getEditor();
    editor.appendChild(document.createTextNode("Compare "));
    editor.appendChild(createPillElement("abc-123", "Alice", "person"));
    editor.appendChild(document.createTextNode(" in "));
    editor.appendChild(createPillElement("team-456", "Platform", "team"));
    fireEvent.input(editor);

    fireEvent.keyDown(editor, { key: "Enter", shiftKey: false });

    expect(onSubmit).toHaveBeenCalledWith("Compare Alice in Platform", [
      { id: "abc-123", name: "Alice", type: "person" },
      { id: "team-456", name: "Platform", type: "team" },
    ]);
  });
});
