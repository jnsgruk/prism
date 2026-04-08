import { Database, HardDrive } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Card, CardContent } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";

import { useSystemInfo } from "@/views/admin/hooks/use-admin";
import { ApiTokensSection } from "@/views/admin/components/api-tokens-section";
import { ResetDataDialog } from "@/views/admin/components/reset-data-dialog";

const formatBytes = (bytes: number): string => {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
};

const StorageSection = (): React.ReactElement => {
  const { data, isLoading, isError, error } = useSystemInfo();

  if (isLoading) {
    return (
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  }

  if (isError) {
    return (
      <Alert variant="destructive">
        <AlertDescription>
          {error instanceof Error ? error.message : "Failed to load system info"}
        </AlertDescription>
      </Alert>
    );
  }

  const dbSize = Number(data?.databaseSizeBytes ?? 0);
  const wsUsed = Number(data?.workspaceUsedBytes ?? 0);
  const wsTotal = Number(data?.workspaceTotalBytes ?? 0);
  const wsPercent = wsTotal > 0 ? Math.round((wsUsed / wsTotal) * 100) : 0;

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <Card>
          <CardContent className="flex items-center gap-3 pt-6">
            <Database className="size-5 text-muted-foreground" />
            <div>
              <p className="text-sm text-muted-foreground">Database</p>
              <p className="text-lg font-semibold tabular-nums">{formatBytes(dbSize)}</p>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="flex items-center gap-3 pt-6">
            <HardDrive className="size-5 text-muted-foreground" />
            <div>
              <p className="text-sm text-muted-foreground">Workspace Used</p>
              <p className="text-lg font-semibold tabular-nums">{formatBytes(wsUsed)}</p>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="flex items-center gap-3 pt-6">
            <HardDrive className="size-5 text-muted-foreground" />
            <div>
              <p className="text-sm text-muted-foreground">Workspace Total</p>
              <p className="text-lg font-semibold tabular-nums">{formatBytes(wsTotal)}</p>
            </div>
          </CardContent>
        </Card>
      </div>
      {wsTotal > 0 && (
        <div className="space-y-1">
          <div className="flex justify-between text-sm text-muted-foreground">
            <span>Workspace usage</span>
            <span className="tabular-nums">{wsPercent}%</span>
          </div>
          <Progress value={wsPercent} />
        </div>
      )}
    </div>
  );
};

export const SystemTab = (): React.ReactElement => (
  <div className="space-y-6 pt-4">
    <p className="text-sm text-muted-foreground">
      System-wide settings, storage usage, API tokens, and destructive operations.
    </p>

    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-medium">Storage</h3>
        <Separator className="mt-2" />
      </div>
      <StorageSection />
    </div>

    <ApiTokensSection />

    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-medium">Danger Zone</h3>
        <Separator className="mt-2" />
      </div>
      <div className="flex items-center justify-between rounded-lg border border-destructive/30 p-4">
        <div>
          <p className="text-sm font-medium">Reset all data</p>
          <p className="text-sm text-muted-foreground">
            Permanently delete all contributions, teams, people, and metric snapshots. Source
            configurations will be preserved.
          </p>
        </div>
        <ResetDataDialog />
      </div>
    </div>
  </div>
);
