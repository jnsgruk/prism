import { Button } from "@/components/ui/button";
import { Download, Loader2 } from "lucide-react";

export const WorkspacePdfLoading = (): React.ReactElement => (
  <div className="flex items-center justify-center gap-2 p-6 text-sm text-muted-foreground" role="status">
    <Loader2 className="size-4 animate-spin" aria-hidden="true" />
    Loading PDF…
  </div>
);

export const WorkspacePdfFailure = ({
  message = "This PDF could not be previewed. Download it to view it in another application.",
  onDownload,
}: {
  message?: string;
  onDownload?: () => void;
}): React.ReactElement => (
  <div className="flex flex-col items-center justify-center gap-3 p-6 text-center" role="alert">
    <p className="text-sm text-muted-foreground">{message}</p>
    {onDownload && (
      <Button variant="outline" size="sm" onClick={onDownload}>
        <Download className="size-4" aria-hidden="true" />
        Download
      </Button>
    )}
  </div>
);
