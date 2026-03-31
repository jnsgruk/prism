import { useEffect, useRef, useState } from "react";
import { Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useDownloadArtifact } from "@/views/ask/hooks/use-artifacts";
import { useGetArtifactDownloadUrl } from "@/views/ask/hooks/use-conversations";

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

export const InlineImage = ({ artifact }: { artifact: ArtifactDisplay }): React.ReactElement => {
  const { download, isPending: isDownloading } = useDownloadArtifact();
  const getUrl = useGetArtifactDownloadUrl();
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const fetchedRef = useRef(false);

  useEffect(() => {
    if (fetchedRef.current) return;
    fetchedRef.current = true;

    getUrl.mutate(artifact.id, {
      onSuccess: (data) => {
        fetch(data.downloadUrl)
          .then((res) => res.blob())
          .then((blob) => {
            setImageUrl(URL.createObjectURL(blob));
          })
          .catch(() => setImageUrl(null));
      },
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentionally run once
  }, []);

  return (
    <div className="space-y-2">
      {imageUrl ? (
        <img src={imageUrl} alt={artifact.displayName} className="max-w-lg rounded-lg shadow-sm" />
      ) : (
        <Skeleton className="h-64 w-full max-w-lg rounded-lg" />
      )}
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => download(artifact.id, artifact.displayName)}
          disabled={isDownloading}
        >
          {isDownloading ? (
            <Loader2 className="mr-1.5 size-3.5 animate-spin" />
          ) : (
            <Download className="mr-1.5 size-3.5" />
          )}
          Download
        </Button>
        <span>{formatSize(artifact.sizeBytes)}</span>
        <span>&middot;</span>
        <span>{artifact.contentType?.split("/")[1]?.toUpperCase()}</span>
      </div>
    </div>
  );
};
