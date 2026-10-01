import WorkspacePdfRenderer from "@/views/ask/components/workspace-pdf-renderer";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import type { DocumentProps, PageProps } from "react-pdf";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const originalResizeObserver = globalThis.ResizeObserver;

const pdf = vi.hoisted(() => ({
  documentProps: null as DocumentProps | null,
  pageProps: null as PageProps | null,
  disconnected: vi.fn<() => void>(),
  resize: null as (() => void) | null,
}));

vi.mock("react-pdf", () => ({
  pdfjs: { GlobalWorkerOptions: { workerSrc: "" } },
  Document: (props: DocumentProps): ReactNode => {
    pdf.documentProps = props;
    return <div>{typeof props.children === "function" ? null : props.children}</div>;
  },
  Page: (props: PageProps): ReactNode => {
    pdf.pageProps = props;
    return <div data-testid="pdf-page">Page {props.pageNumber}</div>;
  },
}));

beforeEach(() => {
  pdf.documentProps = null;
  pdf.pageProps = null;
  pdf.disconnected.mockClear();
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(320);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        pdf.resize = callback;
      }
      observe = (): void => {};
      disconnect = pdf.disconnected;
    },
  );
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.stubGlobal("ResizeObserver", originalResizeObserver);
});

const loadDocument = (count = 3): void => {
  // The library boundary is intentionally mocked; only the count is consumed.
  act(() => {
    const callback = pdf.documentProps?.onLoadSuccess;
    if (callback) Reflect.apply(callback, null, [{ numPages: count }]);
  });
};

describe("WorkspacePdfRenderer", () => {
  it("reports the document count and renders only the controlled page with selectable text", () => {
    const onDocumentLoad = vi.fn<() => void>();
    const { rerender } = render(<WorkspacePdfRenderer url="blob:one" page={2} onDocumentLoad={onDocumentLoad} />);
    expect(screen.queryByTestId("pdf-page")).not.toBeInTheDocument();
    expect(pdf.documentProps?.suspense).toBe(false);
    expect(pdf.documentProps?.loading).toBeDefined();

    loadDocument();
    expect(onDocumentLoad).toHaveBeenCalledWith(3);
    expect(screen.getAllByTestId("pdf-page")).toHaveLength(1);
    expect(screen.getByText("Page 2")).toBeInTheDocument();
    expect(pdf.pageProps).toMatchObject({ width: 320, scale: 1, renderTextLayer: true, renderAnnotationLayer: false });

    rerender(<WorkspacePdfRenderer url="blob:one" page={3} scale={1.5} onDocumentLoad={onDocumentLoad} />);
    expect(screen.getByText("Page 3")).toBeInTheDocument();
    expect(pdf.pageProps?.width).toBe(320);
    expect(pdf.pageProps?.scale).toBe(1.5);
  });

  it.each([240, 960])("zooms relative to the fitted %i px width, including after resize", (initialWidth) => {
    const width = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(initialWidth);
    const onDocumentLoad = vi.fn<() => void>();
    const { rerender } = render(<WorkspacePdfRenderer url="blob:one" page={1} onDocumentLoad={onDocumentLoad} />);
    loadDocument();
    expect(pdf.pageProps).toMatchObject({ width: initialWidth, scale: 1 });

    rerender(<WorkspacePdfRenderer url="blob:one" page={1} scale={1.25} onDocumentLoad={onDocumentLoad} />);
    expect(pdf.pageProps).toMatchObject({ width: initialWidth, scale: 1.25 });
    const enlargedWidth = Number(pdf.pageProps?.width) * Number(pdf.pageProps?.scale);
    expect(enlargedWidth).toBe(initialWidth * 1.25);

    rerender(<WorkspacePdfRenderer url="blob:one" page={1} scale={0.75} onDocumentLoad={onDocumentLoad} />);
    expect(pdf.pageProps).toMatchObject({ width: initialWidth, scale: 0.75 });
    const reducedWidth = Number(pdf.pageProps?.width) * Number(pdf.pageProps?.scale);
    expect(reducedWidth).toBe(initialWidth * 0.75);

    width.mockReturnValue(initialWidth / 2);
    act(() => pdf.resize?.());
    expect(pdf.pageProps).toMatchObject({ width: initialWidth / 2, scale: 0.75 });
    rerender(<WorkspacePdfRenderer url="blob:one" page={1} scale="fit-width" onDocumentLoad={onDocumentLoad} />);
    expect(pdf.pageProps).toMatchObject({ width: initialWidth / 2, scale: 1 });
  });

  it("measures resizing, skips zero-width rendering, and disconnects on unmount", () => {
    const width = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(0);
    const { unmount } = render(<WorkspacePdfRenderer url="blob:one" page={1} onDocumentLoad={vi.fn<() => void>()} />);
    loadDocument();
    expect(screen.queryByTestId("pdf-page")).not.toBeInTheDocument();

    width.mockReturnValue(480);
    act(() => pdf.resize?.());
    expect(pdf.pageProps?.width).toBe(480);
    unmount();
    expect(pdf.disconnected).toHaveBeenCalledOnce();
  });

  it("clamps out-of-range pages and reports the corrected page to the parent", () => {
    const onPageChange = vi.fn<() => void>();
    render(
      <WorkspacePdfRenderer
        url="blob:one"
        page={20}
        onPageChange={onPageChange}
        onDocumentLoad={vi.fn<() => void>()}
      />,
    );
    loadDocument();
    expect(screen.getByText("Page 3")).toBeInTheDocument();
    expect(onPageChange).toHaveBeenCalledWith(3);
  });

  it.each(["onLoadError", "onSourceError"] as const)("offers download after %s", (callback) => {
    const onDownload = vi.fn<() => void>();
    render(
      <WorkspacePdfRenderer url="blob:broken" page={1} onDocumentLoad={vi.fn<() => void>()} onDownload={onDownload} />,
    );
    act(() => pdf.documentProps?.[callback]?.(new Error("Invalid PDF")));
    expect(screen.getByRole("alert")).toHaveTextContent("empty, invalid, or unsupported");
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(onDownload).toHaveBeenCalledOnce();
    expect(screen.queryByTestId("pdf-page")).not.toBeInTheDocument();
  });

  it("shows a distinct password fallback without prompting for a password", () => {
    render(
      <WorkspacePdfRenderer
        url="blob:encrypted"
        page={1}
        onDocumentLoad={vi.fn<() => void>()}
        onDownload={vi.fn<() => void>()}
      />,
    );
    act(() => pdf.documentProps?.onPassword?.(vi.fn<() => void>(), 1));
    expect(screen.getByRole("alert")).toHaveTextContent("password-protected");
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("rejects documents with no pages", () => {
    const onDocumentLoad = vi.fn<() => void>();
    render(<WorkspacePdfRenderer url="blob:empty" page={1} onDocumentLoad={onDocumentLoad} />);
    loadDocument(0);
    expect(screen.getByRole("alert")).toHaveTextContent("no pages");
    expect(onDocumentLoad).not.toHaveBeenCalled();
  });

  it("offers a render-failure fallback and allows another page to render", () => {
    const onDocumentLoad = vi.fn<() => void>();
    const { rerender } = render(<WorkspacePdfRenderer url="blob:one" page={1} onDocumentLoad={onDocumentLoad} />);
    loadDocument();
    act(() => pdf.pageProps?.onRenderError?.(new Error("Unsupported image")));
    expect(screen.getByRole("alert")).toHaveTextContent("page could not be rendered");

    rerender(<WorkspacePdfRenderer url="blob:one" page={2} onDocumentLoad={onDocumentLoad} />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText("Page 2")).toBeInTheDocument();
  });

  it("uses only local supporting assets", () => {
    render(<WorkspacePdfRenderer url="blob:one" page={1} onDocumentLoad={vi.fn<() => void>()} />);
    expect(pdf.documentProps?.options).toMatchObject({
      cMapUrl: "/pdf-assets/cmaps/",
      cMapPacked: true,
      standardFontDataUrl: "/pdf-assets/standard_fonts/",
      wasmUrl: "/pdf-assets/wasm/",
      iccUrl: "/pdf-assets/iccs/",
    });
  });
});
