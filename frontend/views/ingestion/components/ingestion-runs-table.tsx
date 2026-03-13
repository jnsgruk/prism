import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertCircle, CheckCircle2, Loader2 } from "lucide-react";

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

export const IngestionRunsTable = ({ runs }: { runs: IngestionRun[] }): React.ReactElement => {
  if (runs.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        No ingestion runs yet. Trigger a run from one of the sources above.
      </p>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Source</TableHead>
          <TableHead>Started</TableHead>
          <TableHead>Duration</TableHead>
          <TableHead className="text-right">Items</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>Rate limit waits</TableHead>
          <TableHead>Error</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {runs.map((run) => {
          const runConfig = statusConfig[run.status] ?? defaultStatus;
          return (
            <TableRow key={run.id}>
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
              <TableCell className="text-xs">
                {run.rateLimitWaitsSeconds > 0 ? `${String(run.rateLimitWaitsSeconds)}s` : "—"}
              </TableCell>
              <TableCell className="max-w-48 truncate text-xs text-destructive">
                {run.errorMessage ?? "—"}
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
};
