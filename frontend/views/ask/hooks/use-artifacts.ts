import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";

import { useGetArtifactDownloadUrl } from "@/lib/hooks/use-conversations";

export const useDownloadArtifact = (): {
  download: (artifactId: string, displayName: string) => void;
  isPending: boolean;
} => {
  const getUrl = useGetArtifactDownloadUrl();

  const download = useCallback(
    (artifactId: string, displayName: string): void => {
      getUrl.mutate(artifactId, {
        onSuccess: (data) => {
          // The server returns a data URL (base64-encoded bytes proxied from S3).
          // Convert to a blob and trigger a file download.
          fetch(data.downloadUrl)
            .then((res) => res.blob())
            .then((blob) => {
              const url = URL.createObjectURL(blob);
              const a = document.createElement("a");
              a.href = url;
              a.download = displayName;
              document.body.appendChild(a);
              a.click();
              a.remove();
              URL.revokeObjectURL(url);
            })
            .catch(() => {
              toast.error(`Failed to download ${displayName}`);
            });
        },
        onError: (err) => {
          toast.error(`Failed to download ${displayName}: ${err.message}`);
        },
      });
    },
    [getUrl],
  );

  return { download, isPending: getUrl.isPending };
};

type PreviewState = {
  url: string;
  contentType: string;
  displayName: string;
  sizeBytes: number;
  /** For text content types, the decoded text content. */
  textContent?: string;
};

export const usePreviewArtifact = (): {
  preview: (
    artifactId: string,
    displayName: string,
    contentType: string,
    sizeBytes: bigint | number,
  ) => void;
  isPending: boolean;
  state: PreviewState | null;
  close: () => void;
} => {
  const getUrl = useGetArtifactDownloadUrl();
  const [state, setState] = useState<PreviewState | null>(null);
  const blobUrlRef = useRef<string | null>(null);

  const close = useCallback(() => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
    setState(null);
  }, []);

  const preview = useCallback(
    (
      artifactId: string,
      displayName: string,
      contentType: string,
      sizeBytes: bigint | number,
    ): void => {
      getUrl.mutate(artifactId, {
        onSuccess: (data) => {
          fetch(data.downloadUrl)
            .then((res) => res.blob())
            .then(async (blob) => {
              const url = URL.createObjectURL(blob);
              blobUrlRef.current = url;

              const isText =
                contentType.startsWith("text/") ||
                contentType === "application/json" ||
                contentType === "application/xml";

              let textContent: string | undefined;
              if (isText) {
                textContent = await blob.text();
              }

              setState({
                url,
                contentType,
                displayName,
                sizeBytes: typeof sizeBytes === "bigint" ? Number(sizeBytes) : sizeBytes,
                textContent,
              });
            })
            .catch(() => {
              toast.error(`Failed to preview ${displayName}`);
            });
        },
        onError: (err) => {
          toast.error(`Failed to preview ${displayName}: ${err.message}`);
        },
      });
    },
    [getUrl],
  );

  return { preview, isPending: getUrl.isPending, state, close };
};
