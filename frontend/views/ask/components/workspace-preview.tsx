import { Loader2, Maximize2, X } from "lucide-react";

import { Button } from "@/components/ui/button";

import { type ArtifactDisplay, formatSize, isTextContent } from "@/views/ask/hooks/use-file-tree";
import { CodePreview } from "@/views/ask/components/code-preview";

type PreviewState = {
  artifact: ArtifactDisplay;
  url: string;
  contentType: string;
  textContent?: string;
};

const PreviewContent = ({ state }: { state: PreviewState }): React.ReactElement => {
  if (state.contentType.startsWith("image/")) {
    return (
      <div className="flex items-center justify-center p-2">
        <img
          src={state.url}
          alt={state.artifact.displayName}
          className="max-h-48 rounded object-contain"
        />
      </div>
    );
  }

  if (state.contentType === "application/pdf") {
    return (
      <div className="flex flex-col items-center justify-center gap-2 p-4 text-center">
        <p className="text-xs text-muted-foreground">PDF preview not available in sidebar</p>
        <p className="text-[10px] text-muted-foreground">Use the expand button to view full size</p>
      </div>
    );
  }

  if (isTextContent(state.contentType) && state.textContent !== undefined) {
    return (
      <CodePreview
        code={state.textContent}
        fileName={state.artifact.displayName}
        contentType={state.contentType}
        className="max-h-48 bg-muted"
      />
    );
  }

  return (
    <p className="py-4 text-center text-xs text-muted-foreground">
      Preview not available for this file type.
    </p>
  );
};

export const WorkspacePreview = ({
  state,
  isLoading,
  onExpand,
  onClose,
}: {
  state: PreviewState | null;
  isLoading: boolean;
  onExpand: () => void;
  onClose: () => void;
}): React.ReactElement => {
  if (isLoading) {
    return (
      <div className="flex flex-col">
        <div className="flex items-center justify-between border-b px-2 py-1.5">
          <span className="text-xs text-muted-foreground">Loading...</span>
        </div>
        <div className="flex flex-1 items-center justify-center p-4">
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        </div>
      </div>
    );
  }

  if (!state) return <></>;

  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-1.5 border-b px-2 py-1.5">
        <span className="min-w-0 flex-1 truncate text-xs font-medium">
          {state.artifact.displayName}
        </span>
        <span className="shrink-0 text-[10px] text-muted-foreground">
          {formatSize(state.artifact.sizeBytes)}
        </span>
        <Button variant="ghost" size="icon" className="size-5 shrink-0" onClick={onExpand}>
          <Maximize2 className="size-3" />
        </Button>
        <Button variant="ghost" size="icon" className="size-5 shrink-0" onClick={onClose}>
          <X className="size-3" />
        </Button>
      </div>
      <div className="overflow-auto">
        <PreviewContent state={state} />
      </div>
    </div>
  );
};

export type { PreviewState };
