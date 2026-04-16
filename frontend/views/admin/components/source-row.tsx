import { ConfirmDialog } from "@/components/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useTriggerTeamSync } from "@/lib/hooks/use-ingestion";
import { platformLabel } from "@/lib/proto-display";
import { useTeamSyncStatus } from "@/views/admin/hooks/use-team-sync-status";
import { SOURCE_TYPES, baseSourceType } from "@/views/admin/lib/source-types";
import { Key, Loader2, Plug, RefreshCw, Settings2, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { SourceConfig } from "@ps/api/gen/canonical/prism/v1/config_pb";
import { useDeleteSource, useTestConnection, useUpdateSource } from "@ps/hooks/use-config";

import { EditSourceDialog } from "./edit-source-dialog";

const TeamSyncButton = ({ sourceName }: { sourceName: string }): React.ReactElement => {
  const triggerTeamSync = useTriggerTeamSync();
  const { isRunning } = useTeamSyncStatus(sourceName);

  const isBusy = triggerTeamSync.isPending || isRunning;

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span className="inline-flex">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() =>
                triggerTeamSync.mutate(sourceName, {
                  onSuccess: () => toast.success("Team sync triggered"),
                  onError: (err) => toast.error(err instanceof Error ? err.message : "Sync failed"),
                })
              }
              disabled={isBusy}
            >
              {isBusy ? <Loader2 className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
            </Button>
          </span>
        }
      />
      <TooltipContent>{isRunning ? "Team sync in progress…" : "Sync GitHub teams"}</TooltipContent>
    </Tooltip>
  );
};

export const SourceRow = ({ source }: { source: SourceConfig }): React.ReactElement => {
  const updateSource = useUpdateSource();
  const deleteSource = useDeleteSource();
  const testConnection = useTestConnection();
  const [showEdit, setShowEdit] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const secretEntries = Object.entries(source.secretStatus);
  const allSecretsSet = secretEntries.length > 0 && secretEntries.every(([, set]) => set);
  const base = baseSourceType(source.sourceType);
  const sourceLabel = SOURCE_TYPES.find((t) => t.value === base)?.label ?? platformLabel(source.sourceType);

  const handleToggleEnabled = (): void => {
    updateSource.mutate({ sourceId: source.id, enabled: !source.enabled });
  };

  const handleDelete = (): void => {
    deleteSource.mutate(source.id);
  };

  return (
    <>
      <div className="flex items-center justify-between rounded-lg border px-4 py-3">
        <div className="flex items-center gap-3">
          <Switch
            checked={source.enabled}
            onCheckedChange={handleToggleEnabled}
            disabled={updateSource.isPending}
            aria-label={source.enabled ? "Disable source" : "Enable source"}
          />

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
          {source.sourceType === Platform.GITHUB && <TeamSyncButton sourceName={source.name} />}
          <Button variant="ghost" size="icon-sm" onClick={() => setShowEdit(true)} title="Edit settings">
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
            {testConnection.isPending ? <Loader2 className="size-4 animate-spin" /> : <Plug className="size-4" />}
          </Button>

          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setShowDeleteConfirm(true)}
            disabled={deleteSource.isPending}
            title="Delete source"
            className="hover:text-destructive"
          >
            <Trash2 className="size-4" />
          </Button>
        </div>
      </div>

      <EditSourceDialog source={source} open={showEdit} onOpenChange={setShowEdit} />
      <ConfirmDialog
        open={showDeleteConfirm}
        onOpenChange={setShowDeleteConfirm}
        title={`Delete "${source.name}"?`}
        description="This will permanently remove the source and all associated configuration. This action cannot be undone."
        confirmLabel="Delete"
        onConfirm={handleDelete}
      />
    </>
  );
};
