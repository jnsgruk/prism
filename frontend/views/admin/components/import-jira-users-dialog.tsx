import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { useImportJiraUsers } from "@/views/admin/hooks/use-admin";
import { AlertCircle, Upload } from "lucide-react";
import { useState } from "react";

import { cn } from "@ps/cn";

export const ImportJiraUsersDialog = ({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const importJiraUsers = useImportJiraUsers();
  const [dragActive, setDragActive] = useState(false);

  const handleFile = async (file: File): Promise<void> => {
    const buffer = await file.arrayBuffer();
    importJiraUsers.mutate({
      fileContent: new Uint8Array(buffer),
      sourceName: "jira",
    });
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
          <DialogTitle>Import Jira Users</DialogTitle>
          <DialogDescription>
            Upload a CSV export from Jira Cloud admin (Organization &rarr; Users &rarr; Export users) to map Jira
            account IDs to people by email address.
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
          <p className="mb-1 text-sm font-medium">Drop a CSV file here</p>
          <p className="mb-3 text-xs text-muted-foreground">Expected columns: User id, email, User name, User status</p>
          <Button render={<label className="cursor-pointer" />}>
            Browse Files
            <input type="file" accept=".csv" onChange={handleFileInput} className="hidden" />
          </Button>
        </div>

        {importJiraUsers.isPending && <p className="text-sm text-muted-foreground">Importing...</p>}

        {importJiraUsers.isSuccess && (
          <div className="rounded border border-green-200 bg-green-50 p-4 dark:border-green-900 dark:bg-green-950">
            <p className="text-sm font-medium text-green-800 dark:text-green-200">Import complete</p>
            <ul className="mt-1 text-xs text-green-700 dark:text-green-300">
              <li>{importJiraUsers.data.identitiesMapped} identities mapped</li>
              <li>{importJiraUsers.data.unmatchedUsers} users unmatched</li>
            </ul>
            {importJiraUsers.data.warnings.length > 0 && (
              <div className="mt-2 max-h-40 overflow-y-auto">
                <p className="text-xs font-medium text-amber-700 dark:text-amber-300">Warnings:</p>
                {importJiraUsers.data.warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-600 dark:text-amber-400">
                    {w}
                  </p>
                ))}
              </div>
            )}
          </div>
        )}

        {importJiraUsers.isError && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            {importJiraUsers.error instanceof Error ? importJiraUsers.error.message : "Import failed"}
          </Alert>
        )}
      </DialogContent>
    </Dialog>
  );
};
