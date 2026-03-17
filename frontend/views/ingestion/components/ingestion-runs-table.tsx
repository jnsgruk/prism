import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ColumnDef } from "@tanstack/react-table";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import type { HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { RunDetailDialog } from "@/components/run-detail-dialog";
import { formatDuration, formatTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-utils";
import type { StatusFilter } from "@/lib/run-utils";
import { useCancelHandlerRun } from "@/views/ingestion/hooks/use-ingestion";

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
  const cancelRun = useCancelHandlerRun();

  const handleCancel = useCallback(
    (runId: string) => {
      cancelRun.mutate(runId, {
        onSuccess: () => {
          toast.success("Run cancelled");
          setSelectedRun(null);
        },
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
      });
    },
    [cancelRun],
  );

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
          title={selectedRun.sourceName}
          description="Run details"
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
          onCancel={handleCancel}
          cancelPending={cancelRun.isPending}
        />
      )}
    </section>
  );
};
