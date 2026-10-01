import { Button } from "@/components/ui/button";
import { CodePreview } from "@/views/ask/components/code-preview";
import { WorkspacePdfNavigation } from "@/views/ask/components/workspace-pdf-navigation";
import { WorkspacePdfPreview } from "@/views/ask/components/workspace-pdf-preview";
import { formatSize, isTextContent } from "@/views/ask/hooks/use-file-tree";
import type { PdfNavigation, PreviewState } from "@/views/ask/hooks/workspace-preview.types";
import { Loader2, Maximize2, X } from "lucide-react";

const PreviewContent = ({ state }: { state: PreviewState }): React.ReactElement => {
  if (state.contentType.startsWith("image/")) {
    return (
      <div className="flex items-center justify-center p-2">
        <img src={state.url} alt={state.artifact.displayName} className="max-h-full rounded object-contain" />
      </div>
    );
  }

  if (isTextContent(state.contentType) && state.textContent !== undefined) {
    return (
      <CodePreview
        code={state.textContent}
        fileName={state.artifact.displayName}
        contentType={state.contentType}
        className="max-h-full"
      />
    );
  }

  return <p className="py-4 text-center text-xs text-muted-foreground">Preview not available for this file type.</p>;
};

export const WorkspacePreview = ({
  state,
  isLoading,
  onExpand,
  onClose,
  pdf,
  paused = false,
  onDownload,
}: {
  state: PreviewState | null;
  isLoading: boolean;
  onExpand: () => void;
  onClose: () => void;
  pdf: PdfNavigation;
  paused?: boolean;
  onDownload?: () => void;
}): React.ReactElement => {
  if (isLoading) {
    return (
      <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
        <div className="flex items-center justify-between border-b px-2 py-1.5">
          <span className="text-xs text-muted-foreground" role="status">
            Loading...
          </span>
          <Button variant="ghost" size="icon" className="size-5" aria-label="Close preview" onClick={onClose}>
            <X className="size-3" />
          </Button>
        </div>
        <div className="flex flex-1 items-center justify-center p-4">
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        </div>
      </div>
    );
  }

  if (!state) return <></>;

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
      <div className="flex shrink-0 items-center gap-1.5 border-b px-2 py-1.5">
        <span className="min-w-0 flex-1 truncate text-xs font-medium">{state.artifact.displayName}</span>
        <span className="shrink-0 text-[10px] text-muted-foreground">{formatSize(state.artifact.sizeBytes)}</span>
        <Button variant="ghost" size="icon" className="size-5 shrink-0" aria-label="Expand preview" onClick={onExpand}>
          <Maximize2 className="size-3" />
        </Button>
        <Button variant="ghost" size="icon" className="size-5 shrink-0" aria-label="Close preview" onClick={onClose}>
          <X className="size-3" />
        </Button>
      </div>
      {state.contentType === "application/pdf" && (
        <div className="shrink-0 border-b">
          <WorkspacePdfNavigation {...pdf} />
        </div>
      )}
      <div className="min-h-0 min-w-0 flex-1 overflow-auto">
        {state.contentType === "application/pdf" ? (
          !paused && (
            <WorkspacePdfPreview
              url={state.url}
              page={pdf.page}
              onDocumentLoad={pdf.onDocumentLoad}
              scale="fit-width"
              onDownload={onDownload}
            />
          )
        ) : (
          <PreviewContent state={state} />
        )}
      </div>
    </div>
  );
};

export type { PreviewState };
