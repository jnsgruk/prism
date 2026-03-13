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
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
  );
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
    <Table className="table-fixed">
      <TableHeader>
        <TableRow>
          <TableHead className="w-[20%]">Source</TableHead>
          <TableHead className="w-[15%]">Started</TableHead>
          <TableHead className="w-[10%]">Duration</TableHead>
          <TableHead className="w-[8%] text-right">Items</TableHead>
          <TableHead className="w-[12%]">Status</TableHead>
          <TableHead className="w-[35%]">Error</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {runs.map((run) => {
          const runConfig = statusConfig[run.status] ?? defaultStatus;
          return (
            <TableRow key={run.id}>
              <TableCell className="truncate font-medium">{run.sourceName}</TableCell>
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
              <TableCell className="break-words text-xs text-destructive">
                {run.errorMessage ?? "—"}
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
};
