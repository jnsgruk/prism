import { Download, Eye, FileText, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import { useDownloadArtifact, usePreviewArtifact } from "@/views/ask/hooks/use-artifacts";

/** Minimal artifact shape needed for display — compatible with both
 *  `ArtifactInfo` (streaming) and `ConversationArtifact` (DB). */
type ArtifactDisplay = {
  id: string;
  displayName: string;
  contentType?: string;
  sizeBytes: bigint | number;
};

const formatSize = (bytes: bigint | number): string => {
  const n = typeof bytes === "bigint" ? Number(bytes) : bytes;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
};

const canPreview = (contentType?: string): boolean => {
  if (!contentType) return false;
  return (
    contentType.startsWith("image/") ||
    contentType.startsWith("text/") ||
    contentType === "application/pdf" ||
    contentType === "application/json" ||
    contentType === "application/xml"
  );
};

const PreviewContent = ({
  state,
}: {
  state: { url: string; contentType: string; textContent?: string };
}): React.ReactElement => {
  if (state.contentType.startsWith("image/")) {
    return (
      <div className="flex items-center justify-center">
        <img
          src={state.url}
          alt="Artifact preview"
          className="max-h-[80vh] rounded object-contain"
        />
      </div>
    );
  }

  if (state.contentType === "application/pdf") {
    return (
      <iframe
        src={state.url}
        title="PDF preview"
        sandbox="allow-same-origin"
        className="h-[80vh] w-full rounded border-0"
      />
    );
  }

  if (state.textContent !== undefined) {
    return (
      <pre className="max-h-[80vh] overflow-auto rounded-md bg-muted p-4 text-sm">
        <code>{state.textContent}</code>
      </pre>
    );
  }

  return (
    <p className="py-8 text-center text-sm text-muted-foreground">
      Preview not available for this file type.
    </p>
  );
};

export const ArtifactList = ({
  artifacts,
}: {
  artifacts: ArtifactDisplay[];
}): React.ReactElement | null => {
  const { download, isPending: isDownloading } = useDownloadArtifact();
  const { preview, isPending: isPreviewing, state: previewState, close } = usePreviewArtifact();

  if (artifacts.length === 0) return null;

  return (
    <div className="space-y-1.5">
      <p className="text-xs font-medium text-muted-foreground">Artifacts</p>
      <div className="space-y-1">
        {artifacts.map((artifact) => (
          <div
            key={artifact.id}
            className="flex items-center gap-2 rounded-md border px-3 py-2 text-sm"
          >
            <FileText className="size-4 shrink-0 text-muted-foreground" />
            <span className="min-w-0 flex-1 truncate">{artifact.displayName}</span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {formatSize(artifact.sizeBytes)}
            </span>
            {canPreview(artifact.contentType) && (
              <Button
                variant="ghost"
                size="icon"
                className="size-7 shrink-0"
                onClick={() =>
                  preview(
                    artifact.id,
                    artifact.displayName,
                    artifact.contentType!,
                    artifact.sizeBytes,
                  )
                }
                disabled={isPreviewing}
              >
                {isPreviewing ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <Eye className="size-3.5" />
                )}
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon"
              className="size-7 shrink-0"
              onClick={() => download(artifact.id, artifact.displayName)}
              disabled={isDownloading}
            >
              {isDownloading ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Download className="size-3.5" />
              )}
            </Button>
          </div>
        ))}
      </div>

      <Dialog open={previewState !== null} onOpenChange={(open) => !open && close()}>
        {previewState && (
          <DialogContent className="sm:max-w-4xl">
            <DialogHeader>
              <DialogTitle>{previewState.displayName}</DialogTitle>
              <DialogDescription>
                {previewState.contentType} — {formatSize(previewState.sizeBytes)}
              </DialogDescription>
            </DialogHeader>
            <PreviewContent state={previewState} />
          </DialogContent>
        )}
      </Dialog>
    </div>
  );
};
