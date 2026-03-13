import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { AlertCircle, Upload } from "lucide-react";
import { useState } from "react";

import { cn } from "@ps/cn";

import { useImportDirectory } from "@/views/admin/hooks/use-admin";

export const ImportDirectoryDialog = (): React.ReactElement => {
  const importDirectory = useImportDirectory();
  const [dragActive, setDragActive] = useState(false);
  const [open, setOpen] = useState(false);

  const handleFile = async (file: File): Promise<void> => {
    const buffer = await file.arrayBuffer();
    importDirectory.mutate(new Uint8Array(buffer));
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
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button />}>
        <Upload className="size-4" />
        Import Directory
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Import Directory</DialogTitle>
          <DialogDescription>
            Upload an HTML or JSON directory export to populate teams and people.
          </DialogDescription>
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
          <p className="mb-1 text-sm font-medium">Drop an HTML or JSON file here</p>
          <p className="mb-3 text-xs text-muted-foreground">or click to browse</p>
          <Button render={<label className="cursor-pointer" />}>
            Browse Files
            <input type="file" accept=".html,.json" onChange={handleFileInput} className="hidden" />
          </Button>
        </div>

        {importDirectory.isPending && <p className="text-sm text-muted-foreground">Importing...</p>}

        {importDirectory.isSuccess && (
          <div className="rounded border border-green-200 bg-green-50 p-4 dark:border-green-900 dark:bg-green-950">
            <p className="text-sm font-medium text-green-800 dark:text-green-200">
              Import complete
            </p>
            <ul className="mt-1 text-xs text-green-700 dark:text-green-300">
              <li>{importDirectory.data.peopleImported} people imported</li>
              <li>{importDirectory.data.teamsCreated} teams created</li>
              <li>{importDirectory.data.identitiesMapped} identities mapped</li>
            </ul>
            {importDirectory.data.warnings.length > 0 && (
              <div className="mt-2">
                <p className="text-xs font-medium text-amber-700 dark:text-amber-300">Warnings:</p>
                {importDirectory.data.warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-600 dark:text-amber-400">
                    {w}
                  </p>
                ))}
              </div>
            )}
          </div>
        )}

        {importDirectory.isError && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            {importDirectory.error instanceof Error
              ? importDirectory.error.message
              : "Import failed"}
          </Alert>
        )}
      </DialogContent>
    </Dialog>
  );
};
