import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { RunDetailDialog } from "@/components/run-detail-dialog";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatDuration, formatTimestamp } from "@/lib/format";
import { useNow } from "@/lib/hooks/use-now";
import { defaultStatus, statusConfig } from "@/lib/run-status";
import { StatusBadge } from "@/views/ingestion/components/pipeline-graph";
import { useCancelHandlerRun, useListPipelineRuns } from "@/views/ingestion/hooks/use-ingestion";
import { POLL_INTERVAL_ACTIVE, POLL_INTERVAL_IDLE } from "@/views/ingestion/lib/constants";
import { ChevronDown, ChevronRight, History, Loader2 } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import type { HandlerRun, PipelineRunSummary } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { cn } from "@ps/cn";

type StatusFilter = "all" | "completed" | "failed" | "running";

/** Map pipeline string status to a filter category. */
const pipelineStatusCategory = (status: string): StatusFilter => {
  if (status === "running") return "running";
  if (status === "failed") return "failed";
  return "completed";
};

/** Stage display order. */
const STAGE_ORDER = ["ingestion", "identity_resolution", "metrics", "enrichment", "embedding", "insights"];

const STAGE_LABELS: Record<string, string> = {
  ingestion: "Ingestion",
  identity_resolution: "Identity Resolution",
  metrics: "Metrics",
  enrichment: "Enrichment",
  embedding: "Embedding",
  insights: "Insights",
};

/** Map handler_name to a pipeline stage for grouping. */
const handlerStage = (run: HandlerRun): string => {
  const name = run.handlerName;
  if (name.includes("Ingestion") || name.includes("TeamSync")) return "ingestion";
  if (name.includes("IdentityResolution")) return "identity_resolution";
  if (name.includes("Metrics")) return "metrics";
  if (name.includes("Enrichment")) return "enrichment";
  if (name.includes("Embedding")) return "embedding";
  if (name.includes("Insights")) return "insights";
  return "ingestion";
};

/** Group runs by stage, maintaining stage order. */
const groupRunsByStage = (runs: HandlerRun[]): { stage: string; runs: HandlerRun[] }[] => {
  const groups = new Map<string, HandlerRun[]>();
  for (const run of runs) {
    const stage = handlerStage(run);
    const existing = groups.get(stage);
    if (existing) {
      existing.push(run);
    } else {
      groups.set(stage, [run]);
    }
  }
  return STAGE_ORDER.filter((s) => groups.has(s)).map((s) => ({ stage: s, runs: groups.get(s)! }));
};

/** Total items across all runs in a pipeline. */
const totalItems = (runs: HandlerRun[]): number => runs.reduce((sum, r) => sum + r.itemsCollected, 0);

const PipelineRow = ({
  summary,
  rowNumber,
  isExpanded,
  onToggle,
  onSelectRun,
  nowMs,
}: {
  summary: PipelineRunSummary;
  rowNumber: number;
  isExpanded: boolean;
  onToggle: () => void;
  onSelectRun: (run: HandlerRun) => void;
  nowMs: number;
}): React.ReactElement => {
  const pipeline = summary.pipeline!;
  const runs = summary.runs;
  const groups = useMemo(() => groupRunsByStage(runs), [runs]);
  const items = useMemo(() => totalItems(runs), [runs]);
  const shortId = pipeline.id.slice(0, 8);

  return (
    <>
      {/* Pipeline summary row */}
      <TableRow className="cursor-pointer hover:bg-muted/50" onClick={onToggle}>
        <TableCell className="w-[40%]">
          <div className="flex items-center gap-2">
            {isExpanded ? (
              <ChevronDown className="size-4 shrink-0 text-muted-foreground" />
            ) : (
              <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
            )}
            <span className="font-medium">Run #{rowNumber}</span>
            <span className="text-xs text-muted-foreground">({shortId})</span>
            {runs.length > 0 && (
              <span className="text-xs text-muted-foreground">
                &middot; {runs.length} {runs.length === 1 ? "handler" : "handlers"}
              </span>
            )}
          </div>
        </TableCell>
        <TableCell>
          <span className="text-xs">{formatTimestamp(pipeline.startedAt)}</span>
        </TableCell>
        <TableCell>
          <span className="text-xs">{formatDuration(pipeline.startedAt, pipeline.completedAt, nowMs)}</span>
        </TableCell>
        <TableCell>
          <span className="block text-right tabular-nums">{items.toLocaleString()}</span>
        </TableCell>
        <TableCell>
          <StatusBadge status={pipeline.status} />
        </TableCell>
      </TableRow>

      {/* Expanded detail rows */}
      {isExpanded &&
        (groups.length > 0 ? (
          groups.map((group) => (
            <StageGroup
              key={group.stage}
              stage={group.stage}
              runs={group.runs}
              onSelectRun={onSelectRun}
              nowMs={nowMs}
            />
          ))
        ) : (
          <TableRow className="hover:bg-transparent">
            <TableCell colSpan={5} className="py-4 pl-12 text-center text-sm text-muted-foreground">
              {pipeline.status === "running" ? (
                <span className="flex items-center justify-center gap-2">
                  <Loader2 className="size-3.5 animate-spin" />
                  Waiting for handler runs to start…
                </span>
              ) : (
                "No handler runs recorded."
              )}
            </TableCell>
          </TableRow>
        ))}
    </>
  );
};

const StageGroup = ({
  stage,
  runs,
  onSelectRun,
  nowMs,
}: {
  stage: string;
  runs: HandlerRun[];
  onSelectRun: (run: HandlerRun) => void;
  nowMs: number;
}): React.ReactElement => (
  <>
    {/* Stage header */}
    <TableRow className="bg-muted/30 hover:bg-muted/30">
      <TableCell colSpan={5} className="py-1.5 pl-12">
        <span className="text-xs font-medium text-muted-foreground">{STAGE_LABELS[stage] ?? stage}</span>
      </TableCell>
    </TableRow>
    {/* Handler runs within stage */}
    {runs.map((run) => (
      <HandlerRunRow key={run.id} run={run} onSelect={() => onSelectRun(run)} nowMs={nowMs} />
    ))}
  </>
);

const SOURCE_DISPLAY_NAMES: Record<string, string> = {
  _enrichment: "Enrichment",
  _embedding: "Embedding",
  _system: "System",
};

const displaySourceName = (name: string): string => SOURCE_DISPLAY_NAMES[name] ?? name;

const HandlerRunRow = ({
  run,
  onSelect,
  nowMs,
}: {
  run: HandlerRun;
  onSelect: () => void;
  nowMs: number;
}): React.ReactElement => {
  const cfg = statusConfig[run.status] ?? defaultStatus;

  return (
    <TableRow className="cursor-pointer hover:bg-muted/50" onClick={onSelect}>
      <TableCell className="pl-16">
        <span className="text-sm">{displaySourceName(run.sourceName)}</span>
      </TableCell>
      <TableCell>
        <span className="text-xs">{formatTimestamp(run.startedAt)}</span>
      </TableCell>
      <TableCell>
        <span className="text-xs">{formatDuration(run.startedAt, run.completedAt, nowMs)}</span>
      </TableCell>
      <TableCell>
        <span className="block text-right tabular-nums">{run.itemsCollected.toLocaleString()}</span>
      </TableCell>
      <TableCell>
        <Badge variant={cfg.variant} className="gap-1">
          {cfg.icon}
          {cfg.label}
        </Badge>
      </TableCell>
    </TableRow>
  );
};

export const PipelineRunHistoryPanel = ({ hasActiveRun }: { hasActiveRun: boolean }): React.ReactElement => {
  const { data: pipelines, isLoading } = useListPipelineRuns({
    refetchInterval: hasActiveRun ? POLL_INTERVAL_ACTIVE : POLL_INTERVAL_IDLE,
  });
  const cancelRun = useCancelHandlerRun();

  const [historyOpen, setHistoryOpen] = useState(false);
  const [expandedIds, setExpandedIds] = useState(new Set());
  const nowMs = useNow(1000, historyOpen && hasActiveRun);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);

  const handleCancel = useCallback(
    (runId: string) => {
      cancelRun.mutate(runId, {
        onSuccess: () => toast.success("Run cancelled"),
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
      });
    },
    [cancelRun],
  );

  const toggleExpanded = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const statusCounts = useMemo(() => {
    const counts = { completed: 0, failed: 0, running: 0 };
    for (const p of pipelines ?? []) {
      if (!p.pipeline) continue;
      const cat = pipelineStatusCategory(p.pipeline.status);
      if (cat === "completed") counts.completed++;
      else if (cat === "failed") counts.failed++;
      else if (cat === "running") counts.running++;
    }
    return counts;
  }, [pipelines]);

  const filtered = useMemo(() => {
    if (!pipelines) return [];
    if (statusFilter === "all") return pipelines;
    return pipelines.filter((p) => p.pipeline && pipelineStatusCategory(p.pipeline.status) === statusFilter);
  }, [pipelines, statusFilter]);

  const totalCount = filtered.length;
  const pagePipelines = filtered.slice(pageIndex * pageSize, (pageIndex + 1) * pageSize);
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  if (isLoading) {
    return (
      <div className="flex justify-center py-8">
        <Loader2 className="size-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <Collapsible open={historyOpen} onOpenChange={setHistoryOpen}>
      <Card>
        <CardHeader className="cursor-pointer" onClick={() => setHistoryOpen(!historyOpen)}>
          <CollapsibleTrigger render={<button type="button" className="flex w-full items-center gap-2 text-left" />}>
            {historyOpen ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
            <History className="size-4 text-muted-foreground" />
            <CardTitle className="text-sm font-semibold">Run History</CardTitle>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
              {statusCounts.completed > 0 && (
                <span>
                  <span className="font-medium tabular-nums text-foreground">{statusCounts.completed}</span> completed
                </span>
              )}
              {statusCounts.failed > 0 && (
                <>
                  {statusCounts.completed > 0 && <span>&middot;</span>}
                  <span className="text-destructive">
                    <span className="font-medium tabular-nums">{statusCounts.failed}</span> failed
                  </span>
                </>
              )}
              {statusCounts.running > 0 && (
                <>
                  {(statusCounts.completed > 0 || statusCounts.failed > 0) && <span>&middot;</span>}
                  <span>
                    <span className="font-medium tabular-nums text-foreground">{statusCounts.running}</span> running
                  </span>
                </>
              )}
            </div>
          </CollapsibleTrigger>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="space-y-4 pt-2">
            {/* Status filter */}
            <div className="flex items-center gap-1">
              {(["all", "completed", "failed", "running"] as const).map((f) => (
                <button
                  key={f}
                  className={cn(
                    "rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                    statusFilter === f
                      ? "bg-primary text-primary-foreground"
                      : "bg-muted text-muted-foreground hover:text-foreground",
                  )}
                  onClick={() => {
                    setStatusFilter(f);
                    setPageIndex(0);
                  }}
                >
                  {f === "all" ? "All" : f.charAt(0).toUpperCase() + f.slice(1)}
                </button>
              ))}
            </div>

            <div className="overflow-x-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-[40%]">Pipeline</TableHead>
                    <TableHead>Started</TableHead>
                    <TableHead>Duration</TableHead>
                    <TableHead className="text-right">Items</TableHead>
                    <TableHead>Status</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {pagePipelines.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={5} className="text-center text-muted-foreground">
                        No results.
                      </TableCell>
                    </TableRow>
                  ) : (
                    pagePipelines.map(
                      (summary, i) =>
                        summary.pipeline && (
                          <PipelineRow
                            key={summary.pipeline.id}
                            summary={summary}
                            rowNumber={totalCount - (pageIndex * pageSize + i)}
                            isExpanded={expandedIds.has(summary.pipeline.id)}
                            onToggle={() => toggleExpanded(summary.pipeline!.id)}
                            onSelectRun={setSelectedRun}
                            nowMs={nowMs}
                          />
                        ),
                    )
                  )}
                </TableBody>
              </Table>
            </div>

            <DataTablePagination
              totalCount={totalCount}
              pageSize={pageSize}
              pageIndex={pageIndex}
              hasNextPage={hasNextPage}
              onPageSizeChange={(size) => {
                setPageSize(size);
                setPageIndex(0);
              }}
              onPreviousPage={() => setPageIndex((i) => Math.max(0, i - 1))}
              onNextPage={() => setPageIndex((i) => i + 1)}
            />
          </CardContent>
        </CollapsibleContent>
      </Card>

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          title={displaySourceName(selectedRun.sourceName)}
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
