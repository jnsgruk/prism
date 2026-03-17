import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Loader2, Square } from "lucide-react";

import type { HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import { formatDuration, formatFullTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-utils";

export const RunDetailDialog = ({
  run,
  title,
  description,
  open,
  onOpenChange,
  onCancel,
  cancelPending,
}: {
  run: HandlerRun;
  title: string;
  description: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCancel?: (runId: string) => void;
  cancelPending?: boolean;
}): React.ReactElement => {
  const runConfig = statusConfig[run.status] ?? defaultStatus;
  const isRunning = run.status === "running";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3 text-sm">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <p className="text-xs text-muted-foreground">Status</p>
              <Badge variant={runConfig.variant} className="mt-1 gap-1">
                {runConfig.icon}
                {runConfig.label}
              </Badge>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Method</p>
              <p className="font-medium">{run.handlerMethod}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Items collected</p>
              <p className="font-medium">{run.itemsCollected.toLocaleString()}</p>
            </div>
            {run.sourceName && run.sourceName !== "_system" && (
              <div>
                <p className="text-xs text-muted-foreground">Source</p>
                <p className="font-medium">{run.sourceName}</p>
              </div>
            )}
            <div>
              <p className="text-xs text-muted-foreground">Started</p>
              <p>{formatFullTimestamp(run.startedAt)}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Completed</p>
              <p>{formatFullTimestamp(run.completedAt)}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Duration</p>
              <p>{formatDuration(run.startedAt, run.completedAt)}</p>
            </div>
            {run.rateLimitWaitsSeconds > 0 && (
              <div>
                <p className="text-xs text-muted-foreground">Rate limit waits</p>
                <p>{String(run.rateLimitWaitsSeconds)}s</p>
              </div>
            )}
          </div>
          {run.errorMessage && (
            <div>
              <p className="text-xs text-muted-foreground">Error</p>
              <p className="mt-1 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {run.errorMessage}
              </p>
            </div>
          )}
        </div>
        {isRunning && onCancel && (
          <DialogFooter>
            <Button
              variant="destructive"
              size="sm"
              disabled={cancelPending}
              onClick={() => onCancel(run.id)}
            >
              {cancelPending ? (
                <Loader2 className="mr-1.5 size-3.5 animate-spin" />
              ) : (
                <Square className="mr-1.5 size-3.5" />
              )}
              Cancel Run
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
};
