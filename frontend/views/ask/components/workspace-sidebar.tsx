import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, X } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useGetWorkspaceFile, useListWorkspaceFiles } from "@/lib/hooks/use-conversations";

import {
  type ArtifactDisplay,
  type WorkspaceFileDisplay,
  isTextContent,
  useWorkspaceFileTree,
} from "@/views/ask/hooks/use-file-tree";
import { useResize } from "@/views/ask/hooks/use-resize";
import { WorkspaceTree } from "@/views/ask/components/workspace-tree";
import { WorkspacePreview, type PreviewState } from "@/views/ask/components/workspace-preview";
import { WorkspacePreviewDialog } from "@/views/ask/components/workspace-preview-dialog";

const DEFAULT_WIDTH = 320;
const MIN_WIDTH = 240;
const MAX_WIDTH = 640;
const DEFAULT_PREVIEW_HEIGHT = 240;
const MIN_PREVIEW_HEIGHT = 100;
const MAX_PREVIEW_HEIGHT = 500;

export const WorkspaceSidebar = ({
  open,
  conversationId,
  onClose,
}: {
  open: boolean;
  conversationId?: string;
  onClose: () => void;
}): React.ReactElement => {
  const getWorkspaceFile = useGetWorkspaceFile();

  const { data: workspaceData } = useListWorkspaceFiles(conversationId ?? "");
  const workspaceFiles: WorkspaceFileDisplay[] = useMemo(
    () =>
      (workspaceData?.files ?? []).map((f) => ({
        path: f.path,
        sizeBytes: f.sizeBytes,
        isDirectory: f.isDirectory,
        contentType: f.contentType,
      })),
    [workspaceData?.files],
  );
  const workspaceRoots = useWorkspaceFileTree(workspaceFiles);

  // Resizable width
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  const { onPointerDown: onWidthDragDown } = useResize({
    axis: "horizontal",
    min: MIN_WIDTH,
    max: MAX_WIDTH,
    reverse: true, // Dragging left increases width (sidebar is right-anchored).
    onResize: setWidth,
  });

  // Resizable preview height
  const [previewHeight, setPreviewHeight] = useState(DEFAULT_PREVIEW_HEIGHT);
  const previewRef = useRef<HTMLDivElement>(null);
  const { onPointerDown: onPreviewDragDown } = useResize({
    axis: "vertical",
    min: MIN_PREVIEW_HEIGHT,
    max: MAX_PREVIEW_HEIGHT,
    reverse: true, // Dragging up increases height.
    onResize: setPreviewHeight,
    targetRef: previewRef,
  });

  // Preview state
  const [previewState, setPreviewState] = useState<PreviewState | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const blobUrlRef = useRef<string | null>(null);

  const cleanupBlobUrl = useCallback(() => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
  }, []);

  // Revoke blob URL on unmount to prevent memory leaks.
  useEffect(() => cleanupBlobUrl, [cleanupBlobUrl]);

  const loadPreviewFromUrl = useCallback(
    async (dataUrl: string, artifact: ArtifactDisplay): Promise<void> => {
      const res = await fetch(dataUrl);
      const blob = await res.blob();
      cleanupBlobUrl();
      const url = URL.createObjectURL(blob);
      blobUrlRef.current = url;

      const contentType = artifact.contentType ?? "application/octet-stream";
      let textContent: string | undefined;
      if (isTextContent(contentType)) {
        textContent = await blob.text();
      }

      setPreviewState({ artifact, url, contentType, textContent });
      setPreviewLoading(false);
    },
    [cleanupBlobUrl],
  );

  const handlePreview = useCallback(
    (artifact: ArtifactDisplay) => {
      if (!conversationId) return;
      setSelectedPath(artifact.id);
      setPreviewLoading(true);

      getWorkspaceFile.mutate(
        { conversationId, path: artifact.id },
        {
          onSuccess: (data) => {
            loadPreviewFromUrl(data.downloadUrl, {
              ...artifact,
              contentType: data.contentType,
              sizeBytes: data.sizeBytes,
            }).catch(() => {
              toast.error(`Failed to preview ${artifact.displayName}`);
              setPreviewLoading(false);
            });
          },
          onError: (err) => {
            toast.error(`Failed to preview ${artifact.displayName}: ${err.message}`);
            setPreviewLoading(false);
          },
        },
      );
    },
    [conversationId, getWorkspaceFile, loadPreviewFromUrl],
  );

  const handleDownload = useCallback(
    (artifact: ArtifactDisplay) => {
      if (!conversationId) return;
      getWorkspaceFile.mutate(
        { conversationId, path: artifact.id },
        {
          onSuccess: (data) => {
            fetch(data.downloadUrl)
              .then((res) => res.blob())
              .then((blob) => {
                const url = URL.createObjectURL(blob);
                const a = document.createElement("a");
                a.href = url;
                a.download = artifact.displayName;
                document.body.appendChild(a);
                a.click();
                a.remove();
                URL.revokeObjectURL(url);
              })
              .catch(() => toast.error(`Failed to download ${artifact.displayName}`));
          },
          onError: (err) =>
            toast.error(`Failed to download ${artifact.displayName}: ${err.message}`),
        },
      );
    },
    [conversationId, getWorkspaceFile],
  );

  const closePreview = useCallback(() => {
    cleanupBlobUrl();
    setPreviewState(null);
    setSelectedPath(null);
  }, [cleanupBlobUrl]);

  const handleDialogDownload = useCallback(() => {
    if (previewState) handleDownload(previewState.artifact);
  }, [previewState, handleDownload]);

  const fileCount = workspaceFiles.filter((f) => !f.isDirectory).length;
  const showPreview = previewState || previewLoading;

  return (
    <>
      <div
        className={`relative flex h-full shrink-0 flex-col border-l bg-background transition-[width] duration-200 ${
          open ? "" : "w-0 overflow-hidden border-l-0"
        }`}
        style={open ? { width: `${width}px` } : undefined}
      >
        {/* Drag handle — left edge of sidebar */}
        {open && (
          <div
            className="absolute top-0 left-0 z-20 h-full w-1 cursor-col-resize hover:bg-primary/20 active:bg-primary/30"
            onPointerDown={onWidthDragDown}
            data-current-size={width}
          />
        )}

        {/* Header */}
        <div className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
          <span className="flex-1 text-sm font-medium">Workspace</span>
          {fileCount > 0 && (
            <Badge variant="secondary" className="text-[10px]">
              {fileCount}
            </Badge>
          )}
          <Button variant="ghost" size="icon" className="size-6" onClick={onClose}>
            <X className="size-3.5" />
          </Button>
        </div>

        {/* Content */}
        {workspaceRoots.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-1 p-8">
            <FolderOpen className="size-10 text-muted-foreground" />
            <p className="font-medium">No files yet</p>
            <p className="text-center text-sm text-muted-foreground">
              Files created by the agent will appear here.
            </p>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col">
            {/* File tree */}
            <div className="min-h-0 flex-1 overflow-y-auto">
              <WorkspaceTree
                roots={workspaceRoots}
                selectedPath={selectedPath}
                onPreview={handlePreview}
                onDownload={handleDownload}
              />
            </div>

            {/* Preview pane with drag handle on top edge */}
            {showPreview && (
              <div
                ref={previewRef}
                className="relative shrink-0 border-t"
                style={{ height: `${previewHeight}px` }}
              >
                {/* Drag handle — top edge of preview */}
                <div
                  className="absolute top-0 right-0 left-0 z-20 h-1 cursor-row-resize hover:bg-primary/20 active:bg-primary/30"
                  onPointerDown={onPreviewDragDown}
                  data-current-size={previewHeight}
                />
                <div className="h-full overflow-auto">
                  <WorkspacePreview
                    state={previewState}
                    isLoading={previewLoading}
                    onExpand={() => setDialogOpen(true)}
                    onClose={closePreview}
                  />
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Full-size preview dialog */}
      <WorkspacePreviewDialog
        state={previewState}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onDownload={handleDialogDownload}
      />
    </>
  );
};
