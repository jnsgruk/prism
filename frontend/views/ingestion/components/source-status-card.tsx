import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  AlertCircle,
  CheckCircle2,
  Clock,
  Loader2,
  Pause,
  Play,
  RotateCcw,
  Square,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import type { SourceStatus } from "@ps/api/gen/prism/v1/ingestion_pb";
import { SourceState } from "@ps/api/gen/prism/v1/ingestion_pb";
import { cn } from "@ps/cn";

import { useCancelRun, useTriggerRun } from "@/views/ingestion/hooks/use-ingestion";
import { BackfillDialog } from "./backfill-dialog";

const stateConfig: Record<
  SourceState,
  {
    label: string;
    variant: "default" | "secondary" | "destructive" | "outline";
    icon: React.ReactNode;
  }
> = {
  [SourceState.IDLE]: {
    label: "Idle",
    variant: "secondary",
    icon: <CheckCircle2 className="size-3" />,
  },
  [SourceState.COLLECTING]: {
    label: "Collecting",
    variant: "default",
    icon: <Loader2 className="size-3 animate-spin" />,
  },
  [SourceState.WAITING]: {
    label: "Waiting",
    variant: "outline",
    icon: <Pause className="size-3" />,
  },
  [SourceState.ERROR]: {
    label: "Error",
    variant: "destructive",
    icon: <AlertCircle className="size-3" />,
  },
  [SourceState.UNSPECIFIED]: {
    label: "Unknown",
    variant: "outline",
    icon: <Clock className="size-3" />,
  },
};

const formatTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "Never";
  const date = new Date(Number(ts.seconds) * 1000);
  return date.toLocaleString();
};

const formatRelativeTime = (ts?: { seconds: bigint }): string => {
  if (!ts) return "";
  const now = Date.now();
  const then = Number(ts.seconds) * 1000;
  const diffMs = now - then;
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${String(diffMin)}m ago`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${String(diffHours)}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  return `${String(diffDays)}d ago`;
};

export const SourceStatusCard = ({
  source,
  onAction,
}: {
  source: SourceStatus;
  onAction?: () => void;
}): React.ReactElement => {
  const triggerRun = useTriggerRun();
  const cancelRun = useCancelRun();
  const [showBackfill, setShowBackfill] = useState(false);
  const config = stateConfig[source.state] ?? stateConfig[SourceState.UNSPECIFIED];
  const isCollecting = source.state === SourceState.COLLECTING;

  const handleTriggerRun = (): void => {
    triggerRun.mutate(source.name, {
      onSuccess: () => {
        toast.success(`Run triggered for ${source.name}`);
        onAction?.();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to trigger run"),
    });
  };

  const handleCancelRun = (): void => {
    cancelRun.mutate(source.name, {
      onSuccess: () => {
        toast.success(`Cancelled run for ${source.name}`);
        onAction?.();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel run"),
    });
  };

  return (
    <>
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="text-base">{source.name}</CardTitle>
            <Badge variant={config.variant} className="gap-1">
              {config.icon}
              {config.label}
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground">{source.sourceType}</p>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-2 text-sm">
              <div>
                <p className="text-xs text-muted-foreground">
                  {isCollecting ? "Started" : "Last run"}
                </p>
                <p className={cn("font-medium", !source.lastRun && "text-muted-foreground")}>
                  {source.lastRun ? formatRelativeTime(source.lastRun) : "Never"}
                </p>
                {source.lastRun && (
                  <p className="text-xs text-muted-foreground">{formatTimestamp(source.lastRun)}</p>
                )}
              </div>
              <div>
                <p className="text-xs text-muted-foreground">
                  {isCollecting ? "Items so far" : "Items collected"}
                </p>
                <p className="font-medium">{source.itemsCollected.toLocaleString()}</p>
              </div>
            </div>

            {Object.keys(source.rateLimitInfo).length > 0 && (
              <div className="rounded-md bg-muted px-3 py-2">
                <p className="mb-1 text-xs font-medium text-muted-foreground">Rate limit info</p>
                {Object.entries(source.rateLimitInfo).map(([key, value]) => (
                  <p key={key} className="text-xs">
                    <span className="text-muted-foreground">{key}:</span> {value}
                  </p>
                ))}
              </div>
            )}

            <div className="flex gap-2">
              {isCollecting ? (
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={handleCancelRun}
                  disabled={cancelRun.isPending}
                  className="flex-1"
                >
                  {cancelRun.isPending ? (
                    <Loader2 className="mr-1 size-3 animate-spin" />
                  ) : (
                    <Square className="mr-1 size-3" />
                  )}
                  Cancel
                </Button>
              ) : (
                <>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleTriggerRun}
                    disabled={triggerRun.isPending}
                    className="flex-1"
                  >
                    {triggerRun.isPending ? (
                      <Loader2 className="mr-1 size-3 animate-spin" />
                    ) : (
                      <Play className="mr-1 size-3" />
                    )}
                    Run Now
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setShowBackfill(true)}
                    className="flex-1"
                  >
                    <RotateCcw className="mr-1 size-3" />
                    Backfill
                  </Button>
                </>
              )}
            </div>
          </div>
        </CardContent>
      </Card>

      <BackfillDialog sourceName={source.name} open={showBackfill} onOpenChange={setShowBackfill} />
    </>
  );
};
