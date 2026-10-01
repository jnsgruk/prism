import type { WorkspacePdfPreviewProps } from "@/views/ask/components/workspace-pdf-preview";
import { WorkspacePdfFailure, WorkspacePdfLoading } from "@/views/ask/components/workspace-pdf-status";
import { useEffect, useRef, useState } from "react";
import { Document, Page, pdfjs } from "react-pdf";

import { cn } from "@ps/cn";
import "react-pdf/dist/Page/TextLayer.css";

// Resolve the worker from the same PDF.js dependency as the API imported by React-PDF.
pdfjs.GlobalWorkerOptions.workerSrc = new URL("pdfjs-dist/build/pdf.worker.min.mjs", import.meta.url).toString();

const assetBase = `${import.meta.env.BASE_URL}pdf-assets/`;
const documentOptions = {
  cMapUrl: `${assetBase}cmaps/`,
  cMapPacked: true,
  standardFontDataUrl: `${assetBase}standard_fonts/`,
  wasmUrl: `${assetBase}wasm/`,
  iccUrl: `${assetBase}iccs/`,
};

const WorkspacePdfRenderer = ({
  url,
  page,
  onPageChange,
  onDocumentLoad,
  scale = "fit-width",
  onDownload,
  className,
}: WorkspacePdfPreviewProps): React.ReactElement => {
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [pageCount, setPageCount] = useState(0);
  const [failure, setFailure] = useState<string | null>(null);
  const [failedPage, setFailedPage] = useState<number | null>(null);

  useEffect((): (() => void) | undefined => {
    const element = container.current;
    if (!element) return undefined;

    const measure = (): void => setWidth(Math.max(0, Math.floor(element.clientWidth)));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return (): void => observer.disconnect();
  }, []);

  const selectedPage = Math.max(1, Math.min(Math.floor(page) || 1, pageCount || 1));
  const numericScale = typeof scale === "number" && Number.isFinite(scale) && scale > 0 ? scale : 1;

  useEffect(() => {
    if (pageCount > 0 && selectedPage !== page) onPageChange?.(selectedPage);
  }, [onPageChange, page, pageCount, selectedPage]);

  const handleDocumentLoad = (document: { numPages: number }): void => {
    if (document.numPages < 1) {
      setFailure("This PDF has no pages. Download it to inspect the original file.");
      return;
    }

    setPageCount(document.numPages);
    onDocumentLoad(document.numPages);
  };

  const handleLoadError = (): void => {
    setFailure("This PDF is empty, invalid, or unsupported. Download it to view the original file.");
  };

  const handlePassword = (): void => {
    setFailure("This PDF is password-protected. Download it to open it with a password in another application.");
  };

  return (
    <div ref={container} className={cn("min-h-0 min-w-0 max-w-full overflow-auto", className)}>
      {failure ? (
        <WorkspacePdfFailure message={failure} onDownload={onDownload} />
      ) : (
        <Document
          file={url}
          options={documentOptions}
          suspense={false}
          loading={<WorkspacePdfLoading />}
          error={<WorkspacePdfFailure onDownload={onDownload} />}
          onLoadSuccess={handleDocumentLoad}
          onLoadError={handleLoadError}
          onSourceError={handleLoadError}
          onPassword={handlePassword}
        >
          {pageCount > 0 &&
            width > 0 &&
            (failedPage === selectedPage ? (
              <WorkspacePdfFailure
                message="This PDF page could not be rendered. Try another page or download the original file."
                onDownload={onDownload}
              />
            ) : (
              <Page
                key={`${selectedPage}-${width}-${numericScale}`}
                pageNumber={selectedPage}
                width={scale === "fit-width" ? width : undefined}
                scale={numericScale}
                renderTextLayer
                renderAnnotationLayer={false}
                loading={<WorkspacePdfLoading />}
                error={<WorkspacePdfFailure onDownload={onDownload} />}
                onLoadError={(): void => setFailedPage(selectedPage)}
                onRenderError={(): void => setFailedPage(selectedPage)}
                onRenderTextLayerError={(): void => setFailedPage(selectedPage)}
              />
            ))}
        </Document>
      )}
    </div>
  );
};

export default WorkspacePdfRenderer;
