import { Button } from "@/components/ui/button";
import { saveWorkspaceDownload, workspaceDownloadHref } from "@/views/ask/lib/workspace-path";
import { Code, ConnectError } from "@connectrpc/connect";
import { useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

import { WorkspaceFileAvailability } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { useDownloadWorkspaceFile, useResolveWorkspaceFile } from "@ps/hooks/use-conversations";

export const WorkspaceFileLink = ({
  conversationId,
  path,
  children,
}: {
  conversationId?: string;
  path: string | null;
  children: React.ReactNode;
}): React.ReactElement => {
  const resolution = useResolveWorkspaceFile(conversationId ?? "", path ?? "");
  const download = useDownloadWorkspaceFile();
  const pending = useRef(false);
  const navigate = useNavigate();
  const destination = conversationId && path ? workspaceDownloadHref(conversationId, path) : "";

  useEffect(() => {
    if (resolution.error && ConnectError.from(resolution.error).code === Code.Unauthenticated) {
      navigate("/login", { replace: true, state: { returnTo: destination } });
    }
  }, [resolution.error, navigate, destination]);

  if (!conversationId || !path) return <span>{children} (File unavailable)</span>;
  if (resolution.isPending || resolution.isFetching) return <span>{children} (Checking file…)</span>;
  if (resolution.isError || resolution.data?.availability === WorkspaceFileAvailability.UNAVAILABLE) {
    return (
      <span>
        {children} (Could not verify file){" "}
        <Button variant="link" size="sm" onClick={() => void resolution.refetch()}>
          Retry
        </Button>
      </span>
    );
  }
  if (resolution.data?.availability !== WorkspaceFileAvailability.AVAILABLE)
    return <span>{children} (File unavailable)</span>;

  return (
    <a
      href={destination}
      aria-disabled={download.isPending}
      onClick={(event) => {
        if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        event.preventDefault();
        if (pending.current) return;
        pending.current = true;
        download.mutate(
          { conversationId, path },
          {
            onSuccess: (file) => saveWorkspaceDownload(file.blobUrl, path.split("/").at(-1) ?? path),
            onError: (error) => {
              if (ConnectError.from(error).code === Code.Unauthenticated) {
                navigate("/login", { replace: true, state: { returnTo: destination } });
              }
              toast.error("File download failed. Please retry.");
              void resolution.refetch();
            },
            onSettled: () => {
              pending.current = false;
            },
          },
        );
      }}
    >
      {children}
      {download.isPending ? " (Downloading…)" : ""}
    </a>
  );
};
