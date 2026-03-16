import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ColumnDef } from "@tanstack/react-table";
import { AlertCircle, Ban, CheckCircle2, Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import type { HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";

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

const formatTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "—";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
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

const columns: ColumnDef<HandlerRun, unknown>[] = [
  {
    accessorKey: "sourceName",
    header: "Source",
    cell: ({ row }) => <span className="font-medium">{row.original.sourceName}</span>,
  },
  {
    accessorKey: "startedAt",
    header: "Started",
    cell: ({ row }) => <span className="text-xs">{formatTimestamp(row.original.startedAt)}</span>,
  },
  {
    id: "duration",
    header: "Duration",
    cell: ({ row }) => (
      <span className="text-xs">
        {formatDuration(row.original.startedAt, row.original.completedAt)}
      </span>
    ),
  },
  {
    accessorKey: "itemsCollected",
    header: () => <span className="block text-right">Items</span>,
    cell: ({ row }) => (
      <span className="block text-right tabular-nums">
        {row.original.itemsCollected.toLocaleString()}
      </span>
    ),
  },
  {
    accessorKey: "status",
    header: "Status",
    cell: ({ row }) => {
      const cfg = statusConfig[row.original.status] ?? defaultStatus;
      return (
        <Badge variant={cfg.variant} className="gap-1">
          {cfg.icon}
          {cfg.label}
        </Badge>
      );
    },
  },
];

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

type StatusFilter = "all" | "completed" | "failed" | "cancelled" | "running";

export const RunHistoryPanel = ({
  runs,
  sourceNames,
}: {
  runs: HandlerRun[];
  sourceNames: string[];
}): React.ReactElement => {
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [sourceFilter, setSourceFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);

  // Reset to first page when filters change.
  useEffect(() => {
    setPageIndex(0);
  }, [sourceFilter, statusFilter, pageSize]);

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

  const totalCount = filteredRuns.length;
  const pageRuns = filteredRuns.slice(pageIndex * pageSize, (pageIndex + 1) * pageSize);
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  const handleNextPage = useCallback(() => {
    setPageIndex((i) => i + 1);
  }, []);

  const handlePrevPage = useCallback(() => {
    setPageIndex((i) => Math.max(0, i - 1));
  }, []);

  const handlePageSizeChange = useCallback((size: number) => {
    setPageSize(size);
  }, []);

  return (
    <section>
      <h2 className="mb-3 text-sm font-semibold">Run History</h2>

      <div className="space-y-4">
        {/* Filters — same layout pattern as PeopleTab */}
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-1">
            <Button
              variant={sourceFilter === "all" ? "default" : "outline"}
              size="sm"
              onClick={() => setSourceFilter("all")}
            >
              All sources
            </Button>
            {sourceNames.map((name) => (
              <Button
                key={name}
                variant={sourceFilter === name ? "default" : "outline"}
                size="sm"
                onClick={() => setSourceFilter(name)}
              >
                {name}
              </Button>
            ))}
          </div>
          <div className="flex items-center gap-1">
            <Button
              variant={statusFilter === "all" ? "default" : "outline"}
              size="sm"
              onClick={() => setStatusFilter("all")}
            >
              All
            </Button>
            <Button
              variant={statusFilter === "completed" ? "default" : "outline"}
              size="sm"
              onClick={() => setStatusFilter("completed")}
            >
              Completed
            </Button>
            <Button
              variant={statusFilter === "failed" ? "default" : "outline"}
              size="sm"
              onClick={() => setStatusFilter("failed")}
            >
              Failed
            </Button>
            <Button
              variant={statusFilter === "running" ? "default" : "outline"}
              size="sm"
              onClick={() => setStatusFilter("running")}
            >
              Running
            </Button>
          </div>
        </div>

        <DataTable columns={columns} data={pageRuns} onRowClick={setSelectedRun} />

        <DataTablePagination
          totalCount={totalCount}
          pageSize={pageSize}
          pageIndex={pageIndex}
          hasNextPage={hasNextPage}
          onPageSizeChange={handlePageSizeChange}
          onPreviousPage={handlePrevPage}
          onNextPage={handleNextPage}
        />
      </div>

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
        />
      )}
    </section>
  );
};
