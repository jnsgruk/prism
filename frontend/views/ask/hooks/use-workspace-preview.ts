import { useDownloadWorkspaceFile } from "@/lib/hooks/use-conversations";
import { type ArtifactDisplay, isTextContent } from "@/views/ask/hooks/use-file-tree";
import type { PdfNavigation, PreviewState } from "@/views/ask/hooks/workspace-preview.types";
import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { toast } from "sonner";

type WorkspacePreviewController = {
  state: PreviewState | null;
  isLoading: boolean;
  selectedPath: string | null;
  dialogOpen: boolean;
  setDialogOpen: (open: boolean) => void;
  select: (artifact: ArtifactDisplay) => Promise<void>;
  close: () => void;
  pdf: PdfNavigation;
};

export const useWorkspacePreview = (conversationId: string | undefined, open: boolean): WorkspacePreviewController => {
  const { mutateAsync: download } = useDownloadWorkspaceFile();
  const [state, setState] = useState<PreviewState | null>(null);
  const [isLoading, setLoading] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [page, setPage] = useState(1);
  const [pageCount, setPageCount] = useState<number | null>(null);
  const [context, setContext] = useState({ conversationId, open });
  const activeContext = useRef(context);
  const generation = useRef(0);
  const pendingUrls = useRef(new Set<string>());

  const invalidate = useCallback(() => {
    generation.current += 1;
    for (const url of pendingUrls.current) URL.revokeObjectURL(url);
    pendingUrls.current.clear();
  }, []);

  const close = useCallback(() => {
    invalidate();
    setState(null);
    setLoading(false);
    setSelectedPath(null);
    setDialogOpen(false);
    setPage(1);
    setPageCount(null);
  }, [invalidate]);

  // Reset before consumers render a document from a different workspace.
  if (context.conversationId !== conversationId || context.open !== open) {
    setContext({ conversationId, open });
    setState(null);
    setLoading(false);
    setSelectedPath(null);
    setDialogOpen(false);
    setPage(1);
    setPageCount(null);
  }

  useLayoutEffect(() => {
    // This effect owns pending work for this exact workspace visibility/identity.
    activeContext.current = context;
    return invalidate;
  }, [context, invalidate]);

  // Revoke published URLs only after React has removed their consumers.
  useLayoutEffect(() => {
    const url = state?.url;
    return (): void => {
      if (url) URL.revokeObjectURL(url);
    };
  }, [state?.url]);

  const select = useCallback(
    async (artifact: ArtifactDisplay) => {
      if (!conversationId || !open) return;
      close();
      const request = generation.current;
      const isCurrentRequest = (): boolean =>
        request === generation.current &&
        activeContext.current.conversationId === conversationId &&
        activeContext.current.open;
      setSelectedPath(artifact.id);
      setLoading(true);

      let acquiredUrl: string | undefined;
      try {
        const { blobUrl, contentType } = await download({ conversationId, path: artifact.id });
        if (!isCurrentRequest()) {
          URL.revokeObjectURL(blobUrl);
          return;
        }
        acquiredUrl = blobUrl;
        pendingUrls.current.add(blobUrl);

        let textContent: string | undefined;
        if (isTextContent(contentType)) {
          const response = await fetch(blobUrl);
          textContent = await response.text();
        }
        if (!isCurrentRequest()) return;

        pendingUrls.current.delete(blobUrl);
        setState({ artifact: { ...artifact, contentType }, url: blobUrl, contentType, textContent });
        setLoading(false);
      } catch (error) {
        if (!isCurrentRequest()) return;
        if (acquiredUrl && pendingUrls.current.delete(acquiredUrl)) URL.revokeObjectURL(acquiredUrl);
        setLoading(false);
        toast.error(
          `Failed to preview ${artifact.displayName}: ${error instanceof Error ? error.message : "unknown error"}`,
        );
      }
    },
    [close, conversationId, download, open],
  );

  const onPageChange = useCallback(
    (nextPage: number) => {
      setPage(Math.max(1, Math.min(Math.floor(nextPage), pageCount ?? 1)));
    },
    [pageCount],
  );
  const onDocumentLoad = useCallback((count: number) => {
    setPageCount(count);
    setPage((current) => Math.max(1, Math.min(current, count)));
  }, []);

  return {
    state,
    isLoading,
    selectedPath,
    dialogOpen,
    setDialogOpen,
    select,
    close,
    pdf: { page, pageCount, onPageChange, onDocumentLoad },
  };
};
