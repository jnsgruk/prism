import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight, History } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import type { HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { RunDetailDialog } from "@/components/run-detail-dialog";
import { formatDuration, formatTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-status";
import type { StatusFilter } from "@/lib/run-status";
import { useCancelHandlerRun } from "@/views/ingestion/hooks/use-ingestion";

const SOURCE_DISPLAY_NAMES: Record<string, string> = {
  _enrichment: "Enrichment",
  _system: "System",
};

const displaySourceName = (name: string): string => SOURCE_DISPLAY_NAMES[name] ?? name;

const columns: ColumnDef<HandlerRun, unknown>[] = [
  {
    accessorKey: "sourceName",
    header: "Source",
    cell: ({ row }) => (
      <span className="font-medium">{displaySourceName(row.original.sourceName)}</span>
    ),
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
  const [historyOpen, setHistoryOpen] = useState(false);
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [sourceFilter, setSourceFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);
  const cancelRun = useCancelHandlerRun();

  const statusCounts = useMemo(() => {
    const counts = { completed: 0, failed: 0, running: 0 };
    for (const r of runs) {
      if (r.status === "completed" || r.status === "completed_with_warnings") counts.completed++;
      else if (r.status === "failed") counts.failed++;
      else if (r.status === "running") counts.running++;
    }
    return counts;
  }, [runs]);

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
    <Collapsible open={historyOpen} onOpenChange={setHistoryOpen}>
      <Card>
        <CardHeader className="cursor-pointer" onClick={() => setHistoryOpen(!historyOpen)}>
          <CollapsibleTrigger
            render={<button type="button" className="flex w-full items-center gap-2 text-left" />}
          >
            {historyOpen ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
            <History className="size-4 text-muted-foreground" />
            <CardTitle className="text-base">Run History</CardTitle>
            {statusCounts.completed > 0 && (
              <Badge variant="secondary" className="ml-1">
                {statusCounts.completed} completed
              </Badge>
            )}
            {statusCounts.failed > 0 && (
              <Badge variant="destructive" className="ml-1">
                {statusCounts.failed} failed
              </Badge>
            )}
            {statusCounts.running > 0 && (
              <Badge variant="default" className="ml-1">
                {statusCounts.running} running
              </Badge>
            )}
          </CollapsibleTrigger>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="space-y-4 pt-0">
            {/* Filters */}
            <div className="flex flex-wrap items-center gap-3">
              <Select value={sourceFilter} onValueChange={(v) => setSourceFilter(v ?? "all")}>
                <SelectTrigger size="sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All sources</SelectItem>
                  {sourceNames.map((name) => (
                    <SelectItem key={name} value={name}>
                      {displaySourceName(name)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

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
          </CardContent>
        </CollapsibleContent>
      </Card>

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
    </Collapsible>
  );
};
