import { Button } from "@/components/ui/button";
import { useGetWorkspaceFile } from "@/lib/hooks/use-conversations";
import { WorkspacePreviewDialog } from "@/views/ask/components/workspace-preview-dialog";
import { ImageIcon, Loader2, Maximize2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Inline image that fetches its content from the workspace API.
 * Used inside AnswerContent to resolve workspace-relative image paths
 * (e.g. `![chart](chart.png)` written by the agent).
 */
export const WorkspaceImage = ({
  conversationId,
  path,
  alt,
}: {
  conversationId: string;
  path: string;
  alt?: string;
}): React.ReactElement => {
  const getWorkspaceFile = useGetWorkspaceFile();
  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  const [contentType, setContentType] = useState<string>("image/png");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const blobUrlRef = useRef<string | null>(null);

  const cleanup = useCallback(() => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    setError(false);
    cleanup();

    getWorkspaceFile.mutate(
      { conversationId, path },
      {
        onSuccess: (data) => {
          fetch(data.downloadUrl)
            .then((res) => res.blob())
            .then((blob) => {
              const url = URL.createObjectURL(blob);
              blobUrlRef.current = url;
              setBlobUrl(url);
              setContentType(data.contentType || "image/png");
              setLoading(false);
            })
            .catch(() => {
              setError(true);
              setLoading(false);
            });
        },
        onError: () => {
          setError(true);
          setLoading(false);
        },
      },
    );

    return cleanup;
    // Only re-fetch when conversation or path changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId, path]);

  if (loading) {
    return (
      <div className="my-3 flex items-center gap-2 rounded-md border bg-muted/50 p-4">
        <Loader2 className="size-4 animate-spin text-muted-foreground" />
        <span className="text-sm text-muted-foreground">Loading image...</span>
      </div>
    );
  }

  if (error || !blobUrl) {
    return (
      <div className="my-3 flex items-center gap-2 rounded-md border bg-muted/50 p-4">
        <ImageIcon className="size-4 text-muted-foreground" />
        <span className="text-sm text-muted-foreground">Could not load image: {path}</span>
      </div>
    );
  }

  const fileName = path.split("/").pop() ?? path;

  return (
    <>
      <figure className="group relative my-3">
        <div className="overflow-hidden rounded-md border">
          <img src={blobUrl} alt={alt ?? fileName} className="max-h-[500px] w-full object-contain" />
        </div>
        <div className="absolute right-2 top-2 opacity-0 transition-opacity group-hover:opacity-100">
          <Button variant="secondary" size="icon" className="size-7 shadow-sm" onClick={() => setDialogOpen(true)}>
            <Maximize2 className="size-3.5" />
          </Button>
        </div>
        {alt && <figcaption className="mt-1 text-center text-xs text-muted-foreground">{alt}</figcaption>}
      </figure>

      <WorkspacePreviewDialog
        state={{
          artifact: {
            id: path,
            displayName: fileName,
            contentType,
            sizeBytes: 0,
          },
          url: blobUrl,
          contentType,
        }}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onDownload={() => {
          const a = document.createElement("a");
          a.href = blobUrl;
          a.download = fileName;
          document.body.appendChild(a);
          a.click();
          a.remove();
        }}
      />
    </>
  );
};
