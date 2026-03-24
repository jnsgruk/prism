import { Download, FileText, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { ArtifactInfo } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

import { useDownloadArtifact } from "@/views/ask/hooks/use-artifacts";

const formatSize = (bytes: bigint | number): string => {
  const n = typeof bytes === "bigint" ? Number(bytes) : bytes;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
};

export const ArtifactList = ({
  artifacts,
}: {
  artifacts: ArtifactInfo[];
}): React.ReactElement | null => {
  const { download, isPending } = useDownloadArtifact();

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
            <Button
              variant="ghost"
              size="icon"
              className="size-7 shrink-0"
              onClick={() => download(artifact.id, artifact.displayName)}
              disabled={isPending}
            >
              {isPending ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Download className="size-3.5" />
              )}
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
};
