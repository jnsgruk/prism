import { useCallback, useEffect, useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ColumnDef } from "@tanstack/react-table";

import type { HandlerInfo, HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { RunDetailDialog } from "@/components/run-detail-dialog";
import { formatDuration, formatTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-status";
import type { StatusFilter } from "@/lib/run-status";

const displayName = (name: string): string => name.replace("Handler", "");

const handlerRunColumns: ColumnDef<HandlerRun, unknown>[] = [
  {
    accessorKey: "handlerName",
    header: "Handler",
    cell: ({ row }): React.ReactElement => {
      const source = row.original.sourceName;
      const suffix = source && source !== "_system" ? ` \u2014 ${source}` : "";
      return (
        <div>
          <span className="font-medium">
            {displayName(row.original.handlerName)}.{row.original.handlerMethod}
          </span>
          {suffix && <span className="text-muted-foreground">{suffix}</span>}
        </div>
      );
    },
  },
  {
    accessorKey: "startedAt",
    header: "Started",
    cell: ({ row }): React.ReactElement => (
      <span className="text-xs">{formatTimestamp(row.original.startedAt)}</span>
    ),
  },
  {
    id: "duration",
    header: "Duration",
    cell: ({ row }): React.ReactElement => (
      <span className="text-xs">
        {formatDuration(row.original.startedAt, row.original.completedAt)}
      </span>
    ),
  },
  {
    accessorKey: "itemsCollected",
    header: () => <span className="block text-right">Items</span>,
    cell: ({ row }): React.ReactElement => (
      <span className="block text-right tabular-nums">
        {row.original.itemsCollected.toLocaleString()}
      </span>
    ),
  },
  {
    accessorKey: "status",
    header: "Status",
    cell: ({ row }): React.ReactElement => {
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

export const HandlerRunsTable = ({
  runs,
  handlers,
  onCancelRun,
  cancelPending,
}: {
  runs: HandlerRun[];
  handlers: HandlerInfo[];
  onCancelRun: (runId: string) => void;
  cancelPending: boolean;
}): React.ReactElement => {
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [handlerFilter, setHandlerFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);

  useEffect(() => {
    setPageIndex(0);
  }, [handlerFilter, statusFilter, pageSize]);

  const filteredRuns = useMemo(() => {
    let result = runs;
    if (handlerFilter !== "all") {
      result = result.filter((r) => r.handlerName === handlerFilter);
    }
    if (statusFilter !== "all") {
      result = result.filter((r) => r.status === statusFilter);
    }
    return result;
  }, [runs, handlerFilter, statusFilter]);

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
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-1">
          <Button
            variant={handlerFilter === "all" ? "default" : "outline"}
            size="sm"
            onClick={() => setHandlerFilter("all")}
          >
            All handlers
          </Button>
          {handlers.map((h) => (
            <Button
              key={h.name}
              variant={handlerFilter === h.name ? "default" : "outline"}
              size="sm"
              onClick={() => setHandlerFilter(h.name)}
            >
              {displayName(h.name)}
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
            variant={statusFilter === "completed_with_warnings" ? "default" : "outline"}
            size="sm"
            onClick={() => setStatusFilter("completed_with_warnings")}
          >
            Partial
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

      <DataTable columns={handlerRunColumns} data={pageRuns} onRowClick={setSelectedRun} />

      <DataTablePagination
        totalCount={totalCount}
        pageSize={pageSize}
        pageIndex={pageIndex}
        hasNextPage={hasNextPage}
        onPageSizeChange={handlePageSizeChange}
        onPreviousPage={handlePrevPage}
        onNextPage={handleNextPage}
      />

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          title={`${displayName(selectedRun.handlerName)}.${selectedRun.handlerMethod}`}
          description={
            selectedRun.sourceName === "_system"
              ? "Run details"
              : `Source: ${selectedRun.sourceName}`
          }
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
          onCancel={onCancelRun}
          cancelPending={cancelPending}
        />
      )}
    </div>
  );
};
