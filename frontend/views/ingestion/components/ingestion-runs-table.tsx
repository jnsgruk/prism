import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertCircle, CheckCircle2, Loader2 } from "lucide-react";
import { useState } from "react";

import type { IngestionRun } from "@ps/api/gen/prism/v1/ingestion_pb";

type StatusStyle = {
  label: string;
  variant: "default" | "secondary" | "destructive";
  icon: React.ReactNode;
};

const defaultStatus: StatusStyle = {
  label: "Running",
  variant: "default",
  icon: <Loader2 className="size-3 animate-spin" />,
};

const statusConfig: Record<string, StatusStyle> = {
  completed: {
    label: "Completed",
    variant: "secondary",
    icon: <CheckCircle2 className="size-3" />,
  },
  failed: { label: "Failed", variant: "destructive", icon: <AlertCircle className="size-3" /> },
  running: defaultStatus,
};

const formatTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "—";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
  );
};

const formatFullTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "—";
  return new Date(Number(ts.seconds) * 1000).toLocaleString();
};

const formatDuration = (start?: { seconds: bigint }, end?: { seconds: bigint }): string => {
  if (!start || !end) return "—";
  const diffSec = Number(end.seconds - start.seconds);
  if (diffSec < 60) return `${String(diffSec)}s`;
  const min = Math.floor(diffSec / 60);
  const sec = diffSec % 60;
  return `${String(min)}m ${String(sec)}s`;
};

const RunDetailDialog = ({
  run,
  open,
  onOpenChange,
}: {
  run: IngestionRun;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const runConfig = statusConfig[run.status] ?? defaultStatus;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{run.sourceName}</DialogTitle>
          <DialogDescription>Run details</DialogDescription>
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
              <p className="text-xs text-muted-foreground">Items collected</p>
              <p className="font-medium">{run.itemsCollected.toLocaleString()}</p>
            </div>
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
      </DialogContent>
    </Dialog>
  );
};

export const IngestionRunsTable = ({ runs }: { runs: IngestionRun[] }): React.ReactElement => {
  const [selectedRun, setSelectedRun] = useState<IngestionRun | null>(null);

  if (runs.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        No ingestion runs yet. Trigger a run from one of the sources above.
      </p>
    );
  }

  return (
    <>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Source</TableHead>
            <TableHead>Started</TableHead>
            <TableHead>Duration</TableHead>
            <TableHead className="text-right">Items</TableHead>
            <TableHead>Status</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {runs.map((run) => {
            const runConfig = statusConfig[run.status] ?? defaultStatus;
            return (
              <TableRow key={run.id} className="cursor-pointer" onClick={() => setSelectedRun(run)}>
                <TableCell className="font-medium">{run.sourceName}</TableCell>
                <TableCell className="text-xs">{formatTimestamp(run.startedAt)}</TableCell>
                <TableCell className="text-xs">
                  {formatDuration(run.startedAt, run.completedAt)}
                </TableCell>
                <TableCell className="text-right">{run.itemsCollected.toLocaleString()}</TableCell>
                <TableCell>
                  <Badge variant={runConfig.variant} className="gap-1">
                    {runConfig.icon}
                    {runConfig.label}
                  </Badge>
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
        />
      )}
    </>
  );
};
