import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { AlertCircle, ChevronDown, Download, Plug, Plus, Upload } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { useExportSources, useListSources } from "@ps/hooks/use-config";

import { CreateSourceDialog } from "./create-source-dialog";
import { ImportSourcesDialog } from "./import-sources-dialog";
import { SourceRow } from "./source-row";

export const SourcesTab = (): React.ReactElement => {
  const { data: sources, isLoading, error } = useListSources();
  const exportSources = useExportSources();
  const [createOpen, setCreateOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);

  const handleExportSources = async (): Promise<void> => {
    try {
      const response = await exportSources.mutateAsync();
      const blob = new Blob([new Uint8Array(response.jsonData)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `prism-sources-export-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toast.success("Sources exported");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Export failed");
    }
  };

  return (
    <div className="space-y-4 pt-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">Configure data sources and their credentials.</p>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button />}>
            <Plus className="size-4" />
            Add
            <ChevronDown className="size-3.5" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-52">
            <DropdownMenuItem onClick={() => setCreateOpen(true)}>
              <Plus className="size-4" />
              Add Source
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={handleExportSources}>
              <Download className="size-4" />
              Export Sources
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setImportOpen(true)}>
              <Upload className="size-4" />
              Import Sources
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {isLoading && <p className="text-sm text-muted-foreground">Loading sources...</p>}

      {error && (
        <Alert variant="destructive">
          <AlertCircle className="size-4" />
          Failed to load sources.
        </Alert>
      )}

      {sources && sources.length === 0 && (
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Plug className="mb-3 size-10 text-muted-foreground" />
          <p className="mb-1 font-medium">No sources configured</p>
          <p className="text-sm text-muted-foreground">Add a source to start ingesting data.</p>
        </div>
      )}

      {sources && sources.length > 0 && (
        <div className="space-y-2">
          {sources.map((source) => (
            <SourceRow key={source.id} source={source} />
          ))}
        </div>
      )}

      <CreateSourceDialog open={createOpen} onOpenChange={setCreateOpen} />
      <ImportSourcesDialog open={importOpen} onOpenChange={setImportOpen} />
    </div>
  );
};
