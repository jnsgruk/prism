import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { CodePreview } from "@/views/ask/components/code-preview";
import { WorkspacePdfNavigation } from "@/views/ask/components/workspace-pdf-navigation";
import { WorkspacePdfPreview } from "@/views/ask/components/workspace-pdf-preview";
import { formatSize, isTextContent } from "@/views/ask/hooks/use-file-tree";
import type { PdfNavigation, PreviewState } from "@/views/ask/hooks/workspace-preview.types";
import { Download, ZoomIn, ZoomOut, X, Scan } from "lucide-react";
import { useCallback, useState } from "react";

const ZOOM_LEVELS = [25, 50, 75, 100, 150, 200] as const;

const ImagePreview = ({ src, alt }: { src: string; alt: string }): React.ReactElement => {
  const [zoomIndex, setZoomIndex] = useState(3); // 100% default
  const zoom = ZOOM_LEVELS[zoomIndex] ?? 100;

  const zoomIn = useCallback(() => setZoomIndex((i) => Math.min(i + 1, ZOOM_LEVELS.length - 1)), []);
  const zoomOut = useCallback(() => setZoomIndex((i) => Math.max(i - 1, 0)), []);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-2">
      <div className="flex items-center justify-center gap-2">
        <Button
          variant="outline"
          size="icon"
          className="size-7"
          aria-label="Zoom image out"
          onClick={zoomOut}
          disabled={zoomIndex === 0}
        >
          <ZoomOut className="size-3.5" />
        </Button>
        <span className="w-12 text-center text-xs tabular-nums text-muted-foreground">{zoom}%</span>
        <Button
          variant="outline"
          size="icon"
          className="size-7"
          aria-label="Zoom image in"
          onClick={zoomIn}
          disabled={zoomIndex === ZOOM_LEVELS.length - 1}
        >
          <ZoomIn className="size-3.5" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto rounded-md border">
        <img src={src} alt={alt} className="origin-top-left" style={{ transform: `scale(${zoom / 100})` }} />
      </div>
    </div>
  );
};

const PdfPreview = ({
  state,
  pdf,
  onDownload,
}: {
  state: PreviewState;
  pdf?: PdfNavigation;
  onDownload: () => void;
}): React.ReactElement => {
  const [scale, setScale] = useState<number | "fit-width">("fit-width");
  const [page, setPage] = useState(1);
  const [pageCount, setPageCount] = useState<number | null>(null);
  const navigation = pdf ?? { page, pageCount, onPageChange: setPage, onDocumentLoad: setPageCount };
  const numericScale = scale === "fit-width" ? 1 : scale;

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-2 overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center justify-center gap-x-3 gap-y-1">
        <WorkspacePdfNavigation {...navigation} />
        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon"
            className="size-7"
            aria-label="Zoom PDF out"
            disabled={numericScale <= 0.25}
            onClick={() => setScale(Math.max(0.25, numericScale - 0.25))}
          >
            <ZoomOut className="size-3.5" />
          </Button>
          <span className="w-20 text-center text-xs tabular-nums" aria-label="Zoom relative to fitted width">
            {scale === "fit-width" ? "Fit" : `${Math.round(scale * 100)}% of fit`}
          </span>
          <Button
            variant="outline"
            size="icon"
            className="size-7"
            aria-label="Zoom PDF in"
            disabled={numericScale >= 3}
            onClick={() => setScale(Math.min(3, numericScale + 0.25))}
          >
            <ZoomIn className="size-3.5" />
          </Button>
          <Button variant="outline" size="sm" aria-label="Fit PDF to width" onClick={() => setScale("fit-width")}>
            <Scan className="size-3.5" />
            <span>Fit width</span>
          </Button>
        </div>
      </div>
      <div className="min-h-0 min-w-0 flex-1 overflow-auto rounded-md border">
        <WorkspacePdfPreview
          url={state.url}
          page={navigation.page}
          onDocumentLoad={navigation.onDocumentLoad}
          scale={scale}
          onDownload={onDownload}
        />
      </div>
    </div>
  );
};

export const WorkspacePreviewDialog = ({
  state,
  open,
  onOpenChange,
  onDownload,
  pdf,
}: {
  state: PreviewState | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDownload: () => void;
  pdf?: PdfNavigation;
}): React.ReactElement => (
  <Dialog open={open} onOpenChange={onOpenChange}>
    {state && (
      <DialogContent
        className="flex h-[90dvh] max-h-[calc(100dvh-2rem)] min-h-0 min-w-0 flex-col overflow-hidden sm:max-w-5xl"
        showCloseButton={false}
      >
        <DialogHeader className="min-w-0 shrink-0">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <div className="min-w-0 flex-1">
              <DialogTitle className="truncate" title={state.artifact.displayName}>
                {state.artifact.displayName}
              </DialogTitle>
              <DialogDescription
                className="truncate"
                title={`${state.contentType} — ${formatSize(state.artifact.sizeBytes)}`}
              >
                {state.contentType} — {formatSize(state.artifact.sizeBytes)}
              </DialogDescription>
            </div>
            <Button variant="outline" size="sm" className="shrink-0 gap-1.5" onClick={onDownload}>
              <Download className="size-3.5" />
              Download
            </Button>
            <DialogClose
              render={<Button variant="ghost" size="icon" aria-label="Close preview" className="shrink-0" />}
            >
              <X className="size-4" />
            </DialogClose>
          </div>
        </DialogHeader>

        {state.contentType.startsWith("image/") && <ImagePreview src={state.url} alt={state.artifact.displayName} />}

        {state.contentType === "application/pdf" && open && (
          <PdfPreview key={state.url} state={state} pdf={pdf} onDownload={onDownload} />
        )}

        {isTextContent(state.contentType) && state.textContent !== undefined && (
          <CodePreview
            code={state.textContent}
            fileName={state.artifact.displayName}
            contentType={state.contentType}
            className="min-h-0 flex-1 overflow-auto rounded-md border"
          />
        )}

        {!state.contentType.startsWith("image/") &&
          state.contentType !== "application/pdf" &&
          !(isTextContent(state.contentType) && state.textContent !== undefined) && (
            <p className="py-8 text-center text-sm text-muted-foreground">Preview not available for this file type.</p>
          )}
      </DialogContent>
    )}
  </Dialog>
);
