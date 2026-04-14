import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { AlertCircle, Upload } from "lucide-react";
import { useState } from "react";

import { cn } from "@ps/cn";
import { useImportSources } from "@ps/hooks/use-config";

export const ImportSourcesDialog = ({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const importSources = useImportSources();
  const [dragActive, setDragActive] = useState(false);
  const [replace, setReplace] = useState(false);

  const handleFile = async (file: File): Promise<void> => {
    const buffer = await file.arrayBuffer();
    importSources.mutate({ jsonData: new Uint8Array(buffer), replace });
  };

  const handleDrop = (e: React.DragEvent): void => {
    e.preventDefault();
    setDragActive(false);
    const file = e.dataTransfer.files[0];
    if (file) handleFile(file);
  };

  const handleFileInput = (e: React.ChangeEvent<HTMLInputElement>): void => {
    const file = e.target.files?.[0];
    if (file) handleFile(file);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Import Sources</DialogTitle>
          <DialogDescription>Upload a previously exported Prism sources JSON file.</DialogDescription>
        </DialogHeader>

        <div
          onDragOver={(e) => {
            e.preventDefault();
            setDragActive(true);
          }}
          onDragLeave={() => setDragActive(false)}
          onDrop={handleDrop}
          className={cn(
            "flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-8",
            dragActive ? "border-primary bg-primary/5" : "border-muted-foreground/25",
          )}
        >
          <Upload className="mb-2 size-8 text-muted-foreground" />
          <p className="mb-1 text-sm font-medium">Drop a JSON file here</p>
          <p className="mb-3 text-xs text-muted-foreground">or click to browse</p>
          <Button render={<label className="cursor-pointer" />}>
            Browse Files
            <input type="file" accept=".json" onChange={handleFileInput} className="hidden" />
          </Button>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <Switch size="sm" checked={replace} onCheckedChange={setReplace} />
          Replace existing sources (deletes all current sources and their credentials first)
        </label>

        {importSources.isPending && <p className="text-sm text-muted-foreground">Importing...</p>}

        {importSources.isSuccess && (
          <div className="rounded border border-green-200 bg-green-50 p-4 dark:border-green-900 dark:bg-green-950">
            <p className="text-sm font-medium text-green-800 dark:text-green-200">Import complete</p>
            <ul className="mt-1 text-xs text-green-700 dark:text-green-300">
              <li>{importSources.data.sourcesCreated} sources created</li>
              {importSources.data.sourcesSkipped > 0 && (
                <li>{importSources.data.sourcesSkipped} sources skipped (already exist)</li>
              )}
            </ul>
            {importSources.data.warnings.length > 0 && (
              <div className="mt-2">
                <p className="text-xs font-medium text-amber-700 dark:text-amber-300">Warnings:</p>
                {importSources.data.warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-600 dark:text-amber-400">
                    {w}
                  </p>
                ))}
              </div>
            )}
          </div>
        )}

        {importSources.isError && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            {importSources.error instanceof Error ? importSources.error.message : "Import failed"}
          </Alert>
        )}
      </DialogContent>
    </Dialog>
  );
};
