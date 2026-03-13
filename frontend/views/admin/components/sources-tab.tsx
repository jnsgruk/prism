import { Alert } from "@/components/ui/alert";
import { AlertCircle, Plug } from "lucide-react";

import { useListSources } from "@ps/hooks/use-config";

import { CreateSourceDialog } from "./create-source-dialog";
import { SourceRow } from "./source-row";

export const SourcesTab = (): React.ReactElement => {
  const { data: sources, isLoading, error } = useListSources();

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Configure data sources and their credentials.
        </p>
        <CreateSourceDialog />
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
    </div>
  );
};
