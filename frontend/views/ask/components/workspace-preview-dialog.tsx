import { useCallback, useState } from "react";
import { Download, ZoomIn, ZoomOut } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import { formatSize, isTextContent } from "@/views/ask/hooks/use-file-tree";
import { CodePreview } from "@/views/ask/components/code-preview";
import type { PreviewState } from "@/views/ask/components/workspace-preview";

const ZOOM_LEVELS = [25, 50, 75, 100, 150, 200] as const;

const ImagePreview = ({ src, alt }: { src: string; alt: string }): React.ReactElement => {
  const [zoomIndex, setZoomIndex] = useState(3); // 100% default
  const zoom = ZOOM_LEVELS[zoomIndex] ?? 100;

  const zoomIn = useCallback(
    () => setZoomIndex((i) => Math.min(i + 1, ZOOM_LEVELS.length - 1)),
    [],
  );
  const zoomOut = useCallback(() => setZoomIndex((i) => Math.max(i - 1, 0)), []);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-center gap-2">
        <Button
          variant="outline"
          size="icon"
          className="size-7"
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
          onClick={zoomIn}
          disabled={zoomIndex === ZOOM_LEVELS.length - 1}
        >
          <ZoomIn className="size-3.5" />
        </Button>
      </div>
      <div className="max-h-[70vh] overflow-auto rounded-md border">
        <img
          src={src}
          alt={alt}
          className="origin-top-left"
          style={{ transform: `scale(${zoom / 100})` }}
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
}: {
  state: PreviewState | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDownload: () => void;
}): React.ReactElement => (
  <Dialog open={open} onOpenChange={onOpenChange}>
    {state && (
      <DialogContent className="sm:max-w-5xl">
        <DialogHeader>
          <div className="flex items-center gap-2">
            <div className="min-w-0 flex-1">
              <DialogTitle>{state.artifact.displayName}</DialogTitle>
              <DialogDescription>
                {state.contentType} — {formatSize(state.artifact.sizeBytes)}
              </DialogDescription>
            </div>
            <Button variant="outline" size="sm" className="shrink-0 gap-1.5" onClick={onDownload}>
              <Download className="size-3.5" />
              Download
            </Button>
          </div>
        </DialogHeader>

        {state.contentType.startsWith("image/") && (
          <ImagePreview src={state.url} alt={state.artifact.displayName} />
        )}

        {state.contentType === "application/pdf" && (
          <iframe
            src={state.url}
            title="PDF preview"
            sandbox="allow-same-origin"
            className="h-[75vh] w-full rounded border-0"
          />
        )}

        {isTextContent(state.contentType) && state.textContent !== undefined && (
          <CodePreview
            code={state.textContent}
            fileName={state.artifact.displayName}
            contentType={state.contentType}
            className="max-h-[75vh] bg-muted"
          />
        )}

        {!state.contentType.startsWith("image/") &&
          state.contentType !== "application/pdf" &&
          !(isTextContent(state.contentType) && state.textContent !== undefined) && (
            <p className="py-8 text-center text-sm text-muted-foreground">
              Preview not available for this file type.
            </p>
          )}
      </DialogContent>
    )}
  </Dialog>
);
