import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { decodeWorkspacePath, saveWorkspaceDownload } from "@/views/ask/lib/workspace-path";
import { Code, ConnectError } from "@connectrpc/connect";
import { Loader2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { Link, useLocation, useNavigate, useParams } from "react-router-dom";
import { toast } from "sonner";

import { WorkspaceFileAvailability } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { useDownloadWorkspaceFile, useResolveWorkspaceFile } from "@ps/hooks/use-conversations";

const WorkspaceDownloadPage = (): React.ReactElement => {
  const { conversationId = "" } = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  // Router params decode escapes themselves; read the original path to avoid decoding twice.
  const encodedPath = location.pathname.split("/files/").slice(1).join("/files/");
  const path = decodeWorkspacePath(encodedPath);
  const resolution = useResolveWorkspaceFile(conversationId, path ?? "");
  const download = useDownloadWorkspaceFile();
  const started = useRef<string | null>(null);
  const navigationId = `${location.key}:${conversationId}:${path}`;

  const handleError = (error: Error): void => {
    toast.error("File download failed. Please retry.");
    if (ConnectError.from(error).code === Code.Unauthenticated) {
      navigate("/login", { replace: true, state: { returnTo: location.pathname + location.search + location.hash } });
    }
  };

  const startDownload = (): void => {
    if (!path || download.isPending) return;
    download.mutate(
      { conversationId, path },
      {
        onSuccess: (file) => saveWorkspaceDownload(file.blobUrl, path.split("/").at(-1) ?? path),
        onError: handleError,
      },
    );
  };

  useEffect(() => {
    if (resolution.error && ConnectError.from(resolution.error).code === Code.Unauthenticated) {
      navigate("/login", { replace: true, state: { returnTo: location.pathname + location.search + location.hash } });
    }
    if (
      resolution.data?.availability === WorkspaceFileAvailability.AVAILABLE &&
      !download.isPending &&
      started.current !== navigationId
    ) {
      started.current = navigationId;
      startDownload();
    }
    // The navigation identity guards automatic transfer across rerenders and StrictMode.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resolution.data, resolution.error, navigationId, download.isPending]);

  const unavailable =
    !path ||
    resolution.data?.availability === WorkspaceFileAvailability.MISSING ||
    resolution.data?.availability === WorkspaceFileAvailability.INVALID;
  const verificationError =
    resolution.isError || resolution.data?.availability === WorkspaceFileAvailability.UNAVAILABLE;

  let status: React.ReactNode;
  if (unavailable) {
    status = <p>File unavailable. It may have been deleted.</p>;
  } else if (verificationError) {
    status = (
      <>
        <p>Could not verify file. Check your connection and try again.</p>
        <Button onClick={() => void resolution.refetch()}>Retry verification</Button>
      </>
    );
  } else if (resolution.isPending || download.isPending) {
    status = (
      <p className="flex items-center gap-2">
        <Loader2 className="size-4 animate-spin" />
        {download.isPending ? "Downloading file…" : "Checking file…"}
      </p>
    );
  } else {
    let message = "File ready to download.";
    if (download.isError) message = "Download failed. No partial file was saved.";
    else if (download.isSuccess) message = "Download complete.";
    status = (
      <>
        <p>{message}</p>
        <Button onClick={startDownload}>{download.isError ? "Retry download" : "Download again"}</Button>
      </>
    );
  }

  return (
    <>
      <PageHeader title="Download workspace file" />
      <div className="min-w-0 flex-1 space-y-4 overflow-hidden p-6">
        <p className="break-all font-medium">{path ?? "Invalid file path"}</p>
        {status}
        <Button variant="link" render={<Link to={`/ask/${encodeURIComponent(conversationId)}`} />}>
          Back to conversation
        </Button>
      </div>
    </>
  );
};

export default WorkspaceDownloadPage;
