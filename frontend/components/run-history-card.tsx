import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight, History } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import type { HandlerRun } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { RunDetailDialog } from "@/components/run-detail-dialog";
import { StatusFilterButtons } from "@/components/status-filter-buttons";
import type { StatusFilter } from "@/lib/run-status";

type RunHistoryCardProps = {
  /** All runs (pre-filtered by entity if applicable). */
  runs: HandlerRun[];
  columns: ColumnDef<HandlerRun, unknown>[];
  /** Entity filter dropdown rendered in the filter bar. */
  entityDropdown?: React.ReactNode;
  /** Current entity filter value — used to reset pagination when it changes. */
  entityFilter?: string;
  /** Title for the run detail dialog. */
  runTitle: (run: HandlerRun) => string;
  /** Description for the run detail dialog. */
  runDescription?: (run: HandlerRun) => string;
  onCancel: (runId: string) => void;
  cancelPending: boolean;
  /** When true, "All" status hides running entries (default: false). */
  excludeRunningByDefault?: boolean;
};

export const RunHistoryCard = ({
  runs,
  columns,
  entityDropdown,
  entityFilter,
  runTitle,
  runDescription,
  onCancel,
  cancelPending,
  excludeRunningByDefault = false,
}: RunHistoryCardProps): React.ReactElement => {
  const [historyOpen, setHistoryOpen] = useState(false);
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);

  const statusCounts = useMemo(() => {
    const counts = { completed: 0, failed: 0, running: 0 };
    for (const r of runs) {
      if (r.status === "completed" || r.status === "completed_with_warnings") counts.completed++;
      else if (r.status === "failed") counts.failed++;
      else if (r.status === "running") counts.running++;
    }
    return counts;
  }, [runs]);

  useEffect(() => {
    setPageIndex(0);
  }, [entityFilter, statusFilter, pageSize]);

  const filteredRuns = useMemo(() => {
    let result = runs;
    if (statusFilter !== "all") {
      result = result.filter((r) => r.status === statusFilter);
    } else if (excludeRunningByDefault) {
      result = result.filter((r) => r.status !== "running");
    }
    return result;
  }, [runs, statusFilter, excludeRunningByDefault]);

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
            <CardTitle className="text-sm font-semibold">Run History</CardTitle>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
              {statusCounts.completed > 0 && (
                <span>
                  <span className="font-medium tabular-nums text-foreground">
                    {statusCounts.completed}
                  </span>{" "}
                  completed
                </span>
              )}
              {statusCounts.failed > 0 && (
                <>
                  {statusCounts.completed > 0 && <span>·</span>}
                  <span className="text-destructive">
                    <span className="font-medium tabular-nums">{statusCounts.failed}</span> failed
                  </span>
                </>
              )}
              {statusCounts.running > 0 && (
                <>
                  {(statusCounts.completed > 0 || statusCounts.failed > 0) && <span>·</span>}
                  <span>
                    <span className="font-medium tabular-nums text-foreground">
                      {statusCounts.running}
                    </span>{" "}
                    running
                  </span>
                </>
              )}
            </div>
          </CollapsibleTrigger>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="space-y-4 pt-2">
            {/* Filters */}
            <div className="flex flex-wrap items-center gap-3">
              {entityDropdown}
              <StatusFilterButtons value={statusFilter} onChange={setStatusFilter} />
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
          title={runTitle(selectedRun)}
          description={runDescription?.(selectedRun) ?? "Run details"}
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
          onCancel={onCancel}
          cancelPending={cancelPending}
        />
      )}
    </Collapsible>
  );
};
