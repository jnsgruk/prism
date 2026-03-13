import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Key, Loader2, Plug, Settings2, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import type { SourceConfig } from "@ps/api/gen/prism/v1/config_pb";
import { useDeleteSource, useTestConnection, useUpdateSource } from "@ps/hooks/use-config";
import { cn } from "@ps/cn";

import { SOURCE_TYPES } from "@/views/admin/lib/source-types";
import { EditSourceDialog } from "./edit-source-dialog";

export const SourceRow = ({ source }: { source: SourceConfig }): React.ReactElement => {
  const updateSource = useUpdateSource();
  const deleteSource = useDeleteSource();
  const testConnection = useTestConnection();
  const [showEdit, setShowEdit] = useState(false);

  const secretEntries = Object.entries(source.secretStatus);
  const allSecretsSet = secretEntries.length > 0 && secretEntries.every(([, set]) => set);
  const sourceLabel =
    SOURCE_TYPES.find((t) => t.value === source.sourceType)?.label ?? source.sourceType;

  const handleToggleEnabled = (): void => {
    updateSource.mutate({ sourceId: source.id, enabled: !source.enabled });
  };

  const handleDelete = (): void => {
    if (confirm(`Delete source "${source.name}"?`)) {
      deleteSource.mutate(source.id);
    }
  };

  return (
    <>
      <div className="flex items-center justify-between rounded-lg border px-4 py-3">
        <div className="flex items-center gap-3">
          <button
            onClick={handleToggleEnabled}
            disabled={updateSource.isPending}
            className={cn(
              "relative h-5 w-9 rounded-full transition-colors",
              source.enabled ? "bg-green-500" : "bg-muted",
            )}
            title={source.enabled ? "Disable source" : "Enable source"}
          >
            <span
              className={cn(
                "absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform",
                source.enabled ? "left-[18px]" : "left-0.5",
              )}
            />
          </button>

          <div>
            <p className="text-sm font-medium">{source.name}</p>
            <div className="flex items-center gap-2">
              <Badge variant="secondary">{sourceLabel}</Badge>
              {allSecretsSet ? (
                <span className="flex items-center gap-1 text-xs text-green-600">
                  <Key className="size-3" /> Configured
                </span>
              ) : (
                <span className="flex items-center gap-1 text-xs text-amber-600">
                  <Key className="size-3" /> Needs secret
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setShowEdit(true)}
            title="Edit settings"
          >
            <Settings2 className="size-4" />
          </Button>

          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() =>
              testConnection.mutate(source.id, {
                onSuccess: (data) => {
                  if (data.success) {
                    toast.success("Connection successful");
                  } else {
                    toast.error(data.errorMessage || "Connection failed");
                  }
                },
                onError: (err) => {
                  toast.error(err instanceof Error ? err.message : "Test failed");
                },
              })
            }
            disabled={testConnection.isPending}
            title="Test connection"
          >
            {testConnection.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Plug className="size-4" />
            )}
          </Button>

          <Button
            variant="ghost"
            size="icon-sm"
            onClick={handleDelete}
            disabled={deleteSource.isPending}
            title="Delete source"
            className="hover:text-destructive"
          >
            <Trash2 className="size-4" />
          </Button>
        </div>
      </div>

      <EditSourceDialog source={source} open={showEdit} onOpenChange={setShowEdit} />
    </>
  );
};
