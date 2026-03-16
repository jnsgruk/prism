import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertCircle, Ban, CheckCircle2, ChevronLeft, ChevronRight, Loader2 } from "lucide-react";
import { useMemo, useState } from "react";

import type { HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

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
  cancelled: { label: "Cancelled", variant: "secondary", icon: <Ban className="size-3" /> },
  running: defaultStatus,
};

const PAGE_SIZE = 15;

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
  run: HandlerRun;
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

export const RunHistoryPanel = ({
  runs,
  sourceNames,
}: {
  runs: HandlerRun[];
  sourceNames: string[];
}): React.ReactElement => {
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [sourceFilter, setSourceFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [page, setPage] = useState(0);

  // Reset page when filters change
  const filteredRuns = useMemo(() => {
    let result = runs;
    if (sourceFilter !== "all") {
      result = result.filter((r) => r.sourceName === sourceFilter);
    }
    if (statusFilter !== "all") {
      result = result.filter((r) => r.status === statusFilter);
    }
    return result;
  }, [runs, sourceFilter, statusFilter]);

  const totalPages = Math.max(1, Math.ceil(filteredRuns.length / PAGE_SIZE));
  const safePage = Math.min(page, totalPages - 1);
  const pageRuns = filteredRuns.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);

  const statusOptions = useMemo(() => {
    const statuses = new Set(runs.map((r) => r.status));
    return [...statuses].toSorted();
  }, [runs]);

  const handleSourceChange = (value: string | null): void => {
    setSourceFilter(value ?? "all");
    setPage(0);
  };

  const handleStatusChange = (value: string | null): void => {
    setStatusFilter(value ?? "all");
    setPage(0);
  };

  return (
    <div className="rounded-lg border bg-card">
      {/* Header with filters */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b px-5 py-3">
        <h2 className="text-sm font-semibold">Run History</h2>
        <div className="flex items-center gap-2">
          <Select value={sourceFilter} onValueChange={handleSourceChange}>
            <SelectTrigger className="h-8 w-[140px] text-xs">
              <SelectValue placeholder="All sources" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All sources</SelectItem>
              {sourceNames.map((name) => (
                <SelectItem key={name} value={name}>
                  {name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={statusFilter} onValueChange={handleStatusChange}>
            <SelectTrigger className="h-8 w-[130px] text-xs">
              <SelectValue placeholder="All statuses" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All statuses</SelectItem>
              {statusOptions.map((status) => (
                <SelectItem key={status} value={status}>
                  {(statusConfig[status] ?? defaultStatus).label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Table */}
      {pageRuns.length === 0 ? (
        <p className="py-10 text-center text-sm text-muted-foreground">
          {runs.length === 0
            ? "No ingestion runs yet. Trigger a run from one of the sources above."
            : "No runs match the current filters."}
        </p>
      ) : (
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
            {pageRuns.map((run) => {
              const runConfig = statusConfig[run.status] ?? defaultStatus;
              return (
                <TableRow
                  key={run.id}
                  className="cursor-pointer"
                  onClick={() => setSelectedRun(run)}
                >
                  <TableCell className="font-medium">{run.sourceName}</TableCell>
                  <TableCell className="text-xs">{formatTimestamp(run.startedAt)}</TableCell>
                  <TableCell className="text-xs">
                    {formatDuration(run.startedAt, run.completedAt)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {run.itemsCollected.toLocaleString()}
                  </TableCell>
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
      )}

      {/* Pagination footer */}
      {filteredRuns.length > PAGE_SIZE && (
        <div className="flex items-center justify-between border-t px-5 py-3">
          <p className="text-xs text-muted-foreground">
            {filteredRuns.length} runs &middot; page {safePage + 1} of {totalPages}
          </p>
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2"
              disabled={safePage === 0}
              onClick={() => setPage(safePage - 1)}
            >
              <ChevronLeft className="size-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2"
              disabled={safePage >= totalPages - 1}
              onClick={() => setPage(safePage + 1)}
            >
              <ChevronRight className="size-4" />
            </Button>
          </div>
        </div>
      )}

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
        />
      )}
    </div>
  );
};
