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
          window.open(data.downloadUrl, "_blank", "noopener,noreferrer");
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
