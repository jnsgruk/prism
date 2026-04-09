import { cleanup, render, screen } from "@testing-library/react";
import { fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FileAttachmentChips, type AttachedFileInfo } from "./file-attachment-chips";

afterEach(cleanup);

describe("FileAttachmentChips", () => {
  it("renders file chips with names and sizes", () => {
    const files: AttachedFileInfo[] = [
      { name: "report.pdf", size: 42000 },
      { name: "chart.png", size: 128000 },
    ];
    render(<FileAttachmentChips files={files} />);
    expect(screen.getByText("report.pdf")).toBeInTheDocument();
    expect(screen.getByText("chart.png")).toBeInTheDocument();
    expect(screen.getByText("41.0 KB")).toBeInTheDocument();
    expect(screen.getByText("125.0 KB")).toBeInTheDocument();
  });

  it("renders remove button when onRemove is provided", () => {
    const files: AttachedFileInfo[] = [{ name: "file.txt", size: 100 }];
    render(<FileAttachmentChips files={files} onRemove={vi.fn()} />);
    expect(screen.getByLabelText("Remove file.txt")).toBeInTheDocument();
  });

  it("does not render remove button when read-only", () => {
    const files: AttachedFileInfo[] = [{ name: "file.txt", size: 100 }];
    render(<FileAttachmentChips files={files} />);
    expect(screen.queryByLabelText("Remove file.txt")).not.toBeInTheDocument();
  });

  it("calls onRemove with correct index", () => {
    const onRemove = vi.fn();
    const files: AttachedFileInfo[] = [
      { name: "first.txt", size: 100 },
      { name: "second.txt", size: 200 },
    ];
    render(<FileAttachmentChips files={files} onRemove={onRemove} />);
    fireEvent.click(screen.getByLabelText("Remove second.txt"));
    expect(onRemove).toHaveBeenCalledWith(1);
  });

  it("renders nothing when files array is empty", () => {
    const { container } = render(<FileAttachmentChips files={[]} />);
    expect(container.innerHTML).toBe("");
  });

  it("extracts filename from path", () => {
    const files: AttachedFileInfo[] = [{ name: "uploads/nested/deep/file.txt", size: 50 }];
    render(<FileAttachmentChips files={files} />);
    expect(screen.getByText("file.txt")).toBeInTheDocument();
  });
});
