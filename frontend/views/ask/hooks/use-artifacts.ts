import { useCallback } from "react";
import { toast } from "sonner";

import { useGetArtifactDownloadUrl } from "@/views/ask/hooks/use-conversations";

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
