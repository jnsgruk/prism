import { WorkspacePreview } from "@/views/ask/components/workspace-preview";
import { WorkspacePreviewDialog } from "@/views/ask/components/workspace-preview-dialog";
import type { PdfNavigation, PreviewState } from "@/views/ask/hooks/workspace-preview.types";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/views/ask/components/workspace-pdf-preview", () => ({
  WorkspacePdfPreview: ({
    page,
    scale,
    onDownload,
  }: {
    page: number;
    scale: number | string;
    onDownload: () => void;
  }): React.ReactElement => (
    <div data-testid="pdf-renderer">
      Rendered page {page}, scale {scale}
      <button onClick={onDownload}>Fallback download</button>
    </div>
  ),
}));
vi.mock("@/views/ask/components/code-preview", () => ({
  CodePreview: ({ code }: { code: string }): React.ReactElement => <pre>{code}</pre>,
}));
const state: PreviewState = {
  artifact: { id: "report.pdf", displayName: "report.pdf", sizeBytes: 123 },
  url: "blob:pdf",
  contentType: "application/pdf",
};
const navigation = (page = 1, pageCount: number | null = 3): PdfNavigation => ({
  page,
  pageCount,
  onPageChange: vi.fn<(page: number) => void>(),
  onDocumentLoad: vi.fn<(count: number) => void>(),
});
afterEach(cleanup);

describe("workspace PDF surfaces", () => {
  it("renders sidebar page controls, expansion and fallback download", () => {
    const pdf = navigation();
    const onExpand = vi.fn<() => void>();
    const onClose = vi.fn<() => void>();
    const onDownload = vi.fn<() => void>();
    render(
      <WorkspacePreview
        state={state}
        isLoading={false}
        pdf={pdf}
        onExpand={onExpand}
        onClose={onClose}
        onDownload={onDownload}
      />,
    );
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Previous PDF page" }).hasAttribute("disabled")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Next PDF page" }));
    expect(pdf.onPageChange).toHaveBeenCalledWith(2);
    fireEvent.click(screen.getByRole("button", { name: "Expand preview" }));
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    fireEvent.click(screen.getByRole("button", { name: "Fallback download" }));
    expect(onExpand).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
    expect(onDownload).toHaveBeenCalledOnce();
  });

  it("disables unknown/boundary navigation and unmounts the paused renderer", () => {
    const { rerender } = render(
      <WorkspacePreview
        state={state}
        isLoading={false}
        pdf={navigation(1, null)}
        onExpand={vi.fn<() => void>()}
        onClose={vi.fn<() => void>()}
      />,
    );
    expect(screen.getByRole("button", { name: "Next PDF page" }).hasAttribute("disabled")).toBe(true);
    rerender(
      <WorkspacePreview
        state={state}
        isLoading={false}
        pdf={navigation(3)}
        paused
        onExpand={vi.fn<() => void>()}
        onClose={vi.fn<() => void>()}
      />,
    );
    expect(screen.getByRole("button", { name: "Next PDF page" }).hasAttribute("disabled")).toBe(true);
    expect(screen.queryByTestId("pdf-renderer")).toBeNull();
    expect(screen.getByRole("button", { name: "Expand preview" })).toBeTruthy();
  });

  it("can close a pending preview and retains image/text branches", () => {
    const close = vi.fn<() => void>();
    const props = { pdf: navigation(), onExpand: vi.fn<() => void>(), onClose: close };
    const { rerender } = render(<WorkspacePreview {...props} state={null} isLoading />);
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(close).toHaveBeenCalledOnce();
    rerender(<WorkspacePreview {...props} state={{ ...state, contentType: "image/png" }} isLoading={false} />);
    expect(screen.getByAltText("report.pdf")).toBeTruthy();
    rerender(
      <WorkspacePreview
        {...props}
        state={{ ...state, contentType: "text/plain", textContent: "hello" }}
        isLoading={false}
      />,
    );
    expect(screen.getByText("hello")).toBeTruthy();
  });

  it("dialog controls shared pages, local zoom/fit, download, and close", () => {
    const pdf = navigation(2);
    const onOpenChange = vi.fn<(open: boolean) => void>();
    const onDownload = vi.fn<() => void>();
    const { rerender } = render(
      <WorkspacePreviewDialog state={state} open pdf={pdf} onOpenChange={onOpenChange} onDownload={onDownload} />,
    );
    expect(screen.getByTestId("pdf-renderer").textContent).toContain("page 2, scale fit-width");
    fireEvent.click(screen.getByRole("button", { name: "Next PDF page" }));
    expect(pdf.onPageChange).toHaveBeenCalledWith(3);
    fireEvent.click(screen.getByRole("button", { name: "Zoom PDF in" }));
    expect(screen.getByTestId("pdf-renderer").textContent).toContain("scale 1.25");
    expect(screen.getByLabelText("Zoom relative to fitted width").textContent).toBe("125% of fit");
    fireEvent.click(screen.getByRole("button", { name: "Fit PDF to width" }));
    expect(screen.getByTestId("pdf-renderer").textContent).toContain("fit-width");
    fireEvent.click(screen.getByRole("button", { name: "Zoom PDF out" }));
    expect(screen.getByTestId("pdf-renderer").textContent).toContain("scale 0.75");
    expect(screen.getByLabelText("Zoom relative to fitted width").textContent).toBe("75% of fit");
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(onDownload).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onOpenChange).toHaveBeenCalledWith(false, expect.anything());
    rerender(
      <WorkspacePreviewDialog
        state={state}
        open={false}
        pdf={pdf}
        onOpenChange={onOpenChange}
        onDownload={onDownload}
      />,
    );
    expect(screen.queryByTestId("pdf-renderer")).toBeNull();
    rerender(
      <WorkspacePreviewDialog state={state} open pdf={pdf} onOpenChange={onOpenChange} onDownload={onDownload} />,
    );
    expect(screen.getByTestId("pdf-renderer").textContent).toContain("fit-width");
  });

  it("keeps the full long filename accessible alongside download and close", () => {
    const name = `${"very-long-report".repeat(16)}.pdf`;
    render(
      <WorkspacePreviewDialog
        state={{ ...state, artifact: { ...state.artifact, displayName: name } }}
        open
        onOpenChange={vi.fn<(open: boolean) => void>()}
        onDownload={vi.fn<() => void>()}
      />,
    );
    expect(screen.getByRole("heading", { name }).getAttribute("title")).toBe(name);
    expect(screen.getByRole("button", { name: "Download" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Close preview" })).toBeTruthy();
  });

  it("supports image and text dialog callers without PDF props", () => {
    const props = { open: true, onOpenChange: vi.fn<(open: boolean) => void>(), onDownload: vi.fn<() => void>() };
    const { rerender } = render(<WorkspacePreviewDialog {...props} state={{ ...state, contentType: "image/png" }} />);
    expect(screen.getByAltText("report.pdf")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Zoom image in" }));
    expect(screen.getByText("150%")).toBeTruthy();
    rerender(
      <WorkspacePreviewDialog {...props} state={{ ...state, contentType: "text/plain", textContent: "source code" }} />,
    );
    expect(screen.getByText("source code")).toBeTruthy();
  });
});
